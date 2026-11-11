use super::{Geometry, Lease, MessageCopy, ReadingOwner};
use crate::nsh::{ProjectionId, Surface};
use gpui_kit::IntoElement;
use gpui_kit::{prelude::*, *};

use nsh_base::text_view::{Block, CodeBlock, CustomNode, Document, Inline, Table, TableCell};
use nsh_platform_gpui::reading::{ReadingIntent, ReadingRequest, ReadingSnapshot};
use std::{
    cell::{Cell, RefCell},
    ops::Range,
    rc::{Rc, Weak},
    sync::Arc,
};
use vega_markdown::{RenderNode, SourceSpan, SourceTextMap};

#[path = "body_opacity.rs"]
mod opacity;

#[path = "source_anchor.rs"]
mod source_anchor;

fn visible_nodes(model: &super::super::model::StreamModel) -> Vec<RenderNode> {
    use super::super::model::{LineKind, SpanStyle, StreamSpan};
    fn inline(spans: &[StreamSpan]) -> Vec<vega_markdown::Inline> {
        spans
            .iter()
            .map(|span| {
                let content = vec![vega_markdown::Inline::Text(span.text.clone())];
                let value = match span.style {
                    SpanStyle::Strong => vega_markdown::Inline::Strong(content),
                    SpanStyle::Emphasis => vega_markdown::Inline::Emphasis(content),
                    SpanStyle::Strikethrough => vega_markdown::Inline::Strikethrough(content),
                    SpanStyle::Code => vega_markdown::Inline::Code(span.text.clone()),
                    _ => vega_markdown::Inline::Text(span.text.clone()),
                };
                if let Some(url) = &span.link {
                    vega_markdown::Inline::Link {
                        url: url.clone(),
                        title: None,
                        spans: vec![value],
                    }
                } else {
                    value
                }
            })
            .collect()
    }
    let mut nodes = Vec::new();
    let mut previous_code = None;
    for line in model.committed_lines.iter().chain(&model.pending_lines) {
        if matches!(line.kind, LineKind::Spacer | LineKind::UserLabel) {
            continue;
        }
        let node = if let Some(code) = &line.code_block {
            if previous_code == Some(code.identity) {
                continue;
            }
            previous_code = Some(code.identity);
            RenderNode::CodeBlock {
                language: code.language.clone(),
                code: code.code.to_string(),
            }
        } else if let Some(table) = &line.table {
            let rows = table
                .rows
                .iter()
                .map(|row| {
                    row.iter()
                        .map(|spans| vega_markdown::TableCell {
                            spans: inline(spans),
                        })
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            RenderNode::Table(vega_markdown::TableBlock {
                alignments: table.alignments.clone(),
                header: rows.first().cloned().unwrap_or_default(),
                rows: rows.into_iter().skip(1).collect(),
            })
        } else {
            let spans = inline(&line.spans);
            match line.kind {
                LineKind::Heading(level) => RenderNode::Heading { level, spans },
                LineKind::Code => RenderNode::CodeBlock {
                    language: None,
                    code: line.spans.iter().map(|span| span.text.as_str()).collect(),
                },
                LineKind::Rule => RenderNode::ThematicBreak,
                _ => RenderNode::Paragraph { spans },
            }
        };
        let node = if line.kind == LineKind::ListItem || !line.marker.is_empty() {
            RenderNode::List(vega_markdown::ListBlock {
                ordered: line
                    .marker
                    .chars()
                    .next()
                    .is_some_and(|ch| ch.is_ascii_digit()),
                start: line
                    .marker
                    .trim_end_matches(['.', ')'])
                    .parse()
                    .unwrap_or(1),
                items: vec![vega_markdown::ListItem {
                    checked: line.checked,
                    children: vec![node],
                }],
            })
        } else {
            node
        };
        let mut node = node;
        for _ in &line.quotes {
            node = RenderNode::BlockQuote {
                children: vec![node],
            };
        }
        nodes.push(node);
    }
    nodes
}

#[derive(Clone)]
pub(crate) struct Fragment {
    id: String,
    text: String,
    range: Range<usize>,
    synthetic: bool,
    code: bool,
}

#[derive(Clone, PartialEq)]
struct ReadingRow {
    id: u64,
    version: u64,
    blocks: Vec<Block>,
    gap_before: f32,
}

fn is_empty_heading(node: &RenderNode) -> bool {
    fn has_span(inline: &vega_markdown::Inline) -> bool {
        use vega_markdown::Inline;
        match inline {
            Inline::Text(text) => !text.is_empty(),
            Inline::Code(_) => true,
            Inline::Strong(children)
            | Inline::Emphasis(children)
            | Inline::Strikethrough(children)
            | Inline::Link {
                spans: children, ..
            } => children.iter().any(has_span),
        }
    }
    matches!(node, RenderNode::Heading { spans, .. } if !spans.iter().any(has_span))
}

fn reading_block_gap(previous: Option<&Block>, current: &Block) -> f32 {
    use vega_theme::Layout;
    match (previous, current) {
        (None, _) => 0.,
        (Some(previous), _)
            if matches!(list_boundary(previous, false), Block::Blockquote { .. })
                || matches!(list_boundary(current, true), Block::Blockquote { .. }) =>
        {
            Layout::MARKDOWN_BLOCK_GAP
        }
        (_, Block::Heading { .. }) => Layout::MARKDOWN_HEADING_BEFORE,
        (Some(Block::Heading { .. }), Block::Paragraph { .. }) => Layout::MARKDOWN_HEADING_AFTER,
        _ => Layout::MARKDOWN_BLOCK_GAP,
    }
}

#[derive(Clone)]
pub(crate) struct Content {
    pub(crate) document: Document,
    rows: Vec<ReadingRow>,
    pub(crate) text: Arc<str>,
    source: Option<Arc<str>>,
    source_prefix: usize,
    source_start: usize,
    source_domain: u64,
    fragments: Vec<Fragment>,
    opacity: opacity::Mapping,
}

impl Default for Content {
    fn default() -> Self {
        Self {
            document: Document::default(),
            rows: Vec::new(),
            text: Arc::from(""),
            source: None,
            source_prefix: 0,
            source_start: 0,
            source_domain: 0,
            fragments: Vec::new(),
            opacity: Default::default(),
        }
    }
}

struct Builder {
    text: String,
    fragments: Vec<Fragment>,
    prefix: String,
    row: u64,
    ordinal: usize,
}

impl Builder {
    fn separator(&mut self, value: &str) {
        self.text.push_str(value);
    }

    fn fragment(&mut self, value: &str, synthetic: bool, code: bool) -> String {
        let id = format!("{}:{}", self.prefix, self.ordinal);
        self.ordinal += 1;
        let start = self.text.len();
        self.text.push_str(value);
        self.fragments.push(Fragment {
            id: id.clone(),
            text: value.to_owned(),
            range: start..self.text.len(),
            synthetic,
            code,
        });
        id
    }

    fn inlines(&mut self, spans: &[vega_markdown::Inline]) -> Vec<Inline> {
        spans
            .iter()
            .map(|span| match span {
                vega_markdown::Inline::Text(value) => {
                    let id = self.fragment(value, false, false);
                    Inline::Custom(
                        CustomNode::new("vega-reading-leaf")
                            .with_text(value)
                            .with_attribute("binding", id),
                    )
                }
                vega_markdown::Inline::Code(value) => {
                    let id = self.fragment(value, false, false);
                    Inline::Custom(
                        CustomNode::new("vega-reading-inline-code")
                            .with_text(value)
                            .with_attribute("binding", id),
                    )
                }
                vega_markdown::Inline::Strong(spans) => Inline::Strong {
                    content: self.inlines(spans),
                },
                vega_markdown::Inline::Emphasis(spans) => Inline::Emphasis {
                    content: self.inlines(spans),
                },
                vega_markdown::Inline::Strikethrough(spans) => Inline::Strikethrough {
                    content: self.inlines(spans),
                },
                vega_markdown::Inline::Link { url, title, spans } => Inline::Link {
                    url: url.clone(),
                    title: title.clone(),
                    content: self.inlines(spans),
                },
            })
            .collect()
    }

    fn contributes_copy_block(node: &RenderNode) -> bool {
        match node {
            RenderNode::ThematicBreak => false,
            RenderNode::BlockQuote { children } => {
                children.iter().any(Self::contributes_copy_block)
            }
            _ => true,
        }
    }

    fn starts_with_list_line(children: &[RenderNode]) -> bool {
        match children.first() {
            Some(RenderNode::Paragraph { .. }) => true,
            Some(RenderNode::BlockQuote { children }) => Self::starts_with_list_line(children),
            Some(RenderNode::List(list)) => list
                .items
                .first()
                .is_some_and(|item| Self::starts_with_list_line(&item.children)),
            _ => false,
        }
    }

    fn ends_with_list_line(children: &[RenderNode], first_is_marker: bool) -> bool {
        let last = children.iter().rposition(Self::contributes_copy_block);
        let only_marked_line = first_is_marker && last == Some(0);
        match last.map(|index| &children[index]) {
            Some(RenderNode::List(list)) => list
                .items
                .last()
                .is_some_and(|item| Self::ends_with_list_line(&item.children, true)),
            Some(RenderNode::BlockQuote { children }) => {
                Self::ends_with_list_line(children, only_marked_line)
            }
            Some(RenderNode::Paragraph { .. }) => only_marked_line,
            _ => false,
        }
    }

    fn blocks(
        &mut self,
        nodes: &[RenderNode],
        list_depth: usize,
        first_is_marker: bool,
    ) -> Vec<Block> {
        let mut has_copy_block = false;
        nodes
            .iter()
            .enumerate()
            .map(|(index, node)| {
                let contributes_text = Self::contributes_copy_block(node);
                if contributes_text && has_copy_block {
                    self.separator(
                        if list_depth > 0
                            && matches!(node, RenderNode::List(_))
                            && Self::ends_with_list_line(&nodes[..index], first_is_marker)
                            && Self::starts_with_list_line(std::slice::from_ref(node))
                        {
                            "\n"
                        } else {
                            "\n\n"
                        },
                    );
                }
                has_copy_block |= contributes_text;
                match node {
                    RenderNode::Paragraph { spans } => Block::Paragraph {
                        content: self.inlines(spans),
                    },
                    RenderNode::Heading { level, spans } => Block::Heading {
                        level: *level,
                        content: self.inlines(spans),
                        anchor: None,
                    },
                    RenderNode::CodeBlock { language, code } => {
                        let id = self.fragment(code, false, true);
                        let mut block = CodeBlock::new(language.clone(), code);
                        block.meta = Some(id);
                        Block::CodeBlock(block)
                    }
                    RenderNode::BlockQuote { children } => Block::Blockquote {
                        children: self.blocks(children, list_depth, first_is_marker && index == 0),
                        alert: None,
                    },
                    RenderNode::List(list) => {
                        let children = list
                            .items
                            .iter()
                            .enumerate()
                            .map(|(index, item)| {
                                if index > 0 {
                                    self.separator(
                                        if Self::ends_with_list_line(
                                            &list.items[index - 1].children,
                                            true,
                                        ) && Self::starts_with_list_line(&item.children)
                                        {
                                            "\n"
                                        } else {
                                            "\n\n"
                                        },
                                    );
                                }
                                self.separator(&"  ".repeat(list_depth));
                                let marker = if list.ordered {
                                    format!("{}.", list.start.saturating_add(index as u64))
                                } else {
                                    "•".to_owned()
                                };
                                let id = self.fragment(&marker, true, false);
                                self.separator(" ");
                                let mut node = CustomNode::new("vega-reading-list-item")
                                    .with_text(marker)
                                    .with_attribute("binding", id);
                                if let Some(checked) = item.checked {
                                    let checkbox = if checked { "[x]" } else { "[ ]" };
                                    node =
                                        node.with_attribute("checkbox", checkbox).with_attribute(
                                            "checkbox-binding",
                                            self.fragment(checkbox, true, false),
                                        );
                                    self.separator(" ");
                                }
                                Block::Custom(node.with_children(self.blocks(
                                    &item.children,
                                    list_depth + 1,
                                    true,
                                )))
                            })
                            .collect();
                        Block::Custom(CustomNode::new("vega-reading-list").with_children(children))
                    }
                    RenderNode::Table(table) => {
                        let mut flattened = Vec::new();
                        super::super::model::flatten_table(self.row, table, &mut flattened);
                        let weights = flattened
                            .first()
                            .and_then(|line| line.table.as_ref())
                            .map(|table| table.column_weights())
                            .unwrap_or_default();
                        let mut node = CustomNode::new("vega-reading-table");
                        for (column, weight) in weights.into_iter().enumerate() {
                            node =
                                node.with_attribute(format!("column-{column}"), weight.to_string());
                        }
                        let mut native = Table::default();
                        native.align = table
                            .alignments
                            .iter()
                            .map(|align| match align {
                                vega_markdown::TableAlignment::None => {
                                    nsh_base::text_view::ColumnAlign::None
                                }
                                vega_markdown::TableAlignment::Left => {
                                    nsh_base::text_view::ColumnAlign::Left
                                }
                                vega_markdown::TableAlignment::Center => {
                                    nsh_base::text_view::ColumnAlign::Center
                                }
                                vega_markdown::TableAlignment::Right => {
                                    nsh_base::text_view::ColumnAlign::Right
                                }
                            })
                            .collect();
                        for (row, cells) in std::iter::once(&table.header)
                            .chain(&table.rows)
                            .enumerate()
                        {
                            if row > 0 {
                                self.separator("\n");
                            }
                            let cells = cells
                                .iter()
                                .enumerate()
                                .map(|(column, cell)| {
                                    if column > 0 {
                                        self.separator("\t");
                                    }
                                    TableCell::new(self.inlines(&cell.spans))
                                })
                                .collect();
                            if row == 0 {
                                native.head = cells;
                            } else {
                                native.rows.push(cells);
                            }
                        }
                        Block::Custom(node.with_children(vec![Block::Table(native)]))
                    }
                    RenderNode::ThematicBreak => Block::Rule,
                }
            })
            .collect()
    }
}

impl Content {
    pub(crate) fn from_model(model: &super::super::model::StreamModel, source: u64) -> Self {
        let mut builder = Builder {
            text: String::new(),
            fragments: Vec::new(),
            prefix: String::new(),
            row: 0,
            ordinal: 0,
        };
        let fallback = if model.reading_nodes.is_empty() {
            vec![(0, model.content_revision(), Arc::new(visible_nodes(model)))]
        } else {
            Vec::new()
        };
        let retained = if model.reading_nodes.is_empty() {
            &fallback
        } else {
            &model.reading_nodes
        };
        let mut blocks = Vec::new();
        let mut rows = Vec::new();
        for (id, version, nodes) in retained {
            let filtered;
            let nodes = if nodes.iter().any(is_empty_heading) {
                filtered = nodes
                    .iter()
                    .filter(|node| !is_empty_heading(node))
                    .cloned()
                    .collect::<Vec<_>>();
                filtered.as_slice()
            } else {
                nodes.as_slice()
            };
            if nodes.is_empty() {
                continue;
            }
            if !blocks.is_empty() {
                builder.separator("\n\n");
            }
            builder.prefix = format!("vega-reading-{source}-{id}-{version}");
            builder.row = *id;
            builder.ordinal = 0;
            let row = builder.blocks(nodes, 0, false);
            let gap_before = row
                .first()
                .map_or(0., |first| reading_block_gap(blocks.last(), first));
            blocks.extend(row.iter().cloned());
            rows.push(ReadingRow {
                id: *id,
                version: *version,
                blocks: row,
                gap_before,
            });
        }
        let mut content = Self {
            document: Document::new(blocks),
            rows,
            text: builder.text.into(),
            source: None,
            source_prefix: 0,
            source_start: 0,
            source_domain: 0,
            fragments: builder.fragments,
            opacity: Default::default(),
        };
        content.opacity = opacity::Mapping::build(&content, model);
        content
    }

    pub(crate) fn with_source(mut self, source: String, prefix: usize, start: usize) -> Self {
        self.source = Some(source.into());
        self.source_prefix = prefix;
        self.source_start = start;
        self
    }

    pub(crate) fn with_source_domain(mut self, domain: u64) -> Self {
        self.source_domain = domain;
        self
    }

    fn source_continues(&self, previous: &Self, source: &str, previous_source: &str) -> bool {
        if self.source_domain != previous.source_domain {
            return false;
        }
        let current = self.source.as_deref().unwrap_or(source);
        let previous_text = previous.source.as_deref().unwrap_or(previous_source);
        let Some(current) = current.get(self.source_prefix..) else {
            return false;
        };
        let Some(previous_text) = previous_text.get(previous.source_prefix..) else {
            return false;
        };
        let start = self.source_start.max(previous.source_start);
        let end =
            (self.source_start + current.len()).min(previous.source_start + previous_text.len());
        if start >= end {
            return previous_text.is_empty();
        }
        current
            .get(start - self.source_start..end - self.source_start)
            .zip(previous_text.get(start - previous.source_start..end - previous.source_start))
            .is_some_and(|(current, previous)| current == previous)
    }

    fn mapping(&self, source: &str) -> Vec<SourceSpan> {
        let mut mapper = SourceTextMap::new(self.source.as_deref().unwrap_or(source));
        let mut spans = Vec::new();
        for fragment in &self.fragments {
            if fragment.synthetic {
                continue;
            }
            if fragment.code {
                let mut offset = fragment.range.start;
                for line in fragment.text.split_inclusive('\n') {
                    let value = line.strip_suffix('\n').unwrap_or(line);
                    spans.extend(mapper.take(value, offset));
                    offset += line.len();
                }
            } else {
                spans.extend(mapper.take(&fragment.text, fragment.range.start));
            }
        }
        spans
            .into_iter()
            .filter_map(|mut span| {
                if span.source.start < self.source_prefix {
                    return None;
                }
                span.source = span.source.start - self.source_prefix + self.source_start
                    ..span.source.end - self.source_prefix + self.source_start;
                Some(span)
            })
            .collect()
    }

    fn code_range_mapped(&self, fragment: &Fragment, mapping: &[SourceSpan]) -> bool {
        let mut offset = fragment.range.start;
        for line in fragment.text.split_inclusive('\n') {
            let value = line.strip_suffix('\n').unwrap_or(line);
            if !value.is_empty()
                && !mapping.iter().any(|span| {
                    span.rendered.start <= offset && span.rendered.end >= offset + value.len()
                })
            {
                return false;
            }
            offset += line.len();
        }
        true
    }

    fn reading_links(&self) -> Vec<(super::super::links::LinkRange, Vec<String>)> {
        fn bindings(spans: &[Inline], output: &mut Vec<String>) {
            for span in spans {
                match span {
                    Inline::Custom(node) => {
                        if let Some(binding) = node.attributes.get("binding") {
                            output.push(binding.clone());
                        }
                    }
                    Inline::Strong { content }
                    | Inline::Emphasis { content }
                    | Inline::Strikethrough { content }
                    | Inline::Link { content, .. } => bindings(content, output),
                    _ => {}
                }
            }
        }
        fn inlines(spans: &[Inline], output: &mut Vec<(String, String, Vec<String>)>) {
            for span in spans {
                match span {
                    Inline::Link { url, content, .. } => {
                        let mut ids = Vec::new();
                        bindings(content, &mut ids);
                        output.push((
                            url.clone(),
                            content.iter().map(Inline::plain_text).collect(),
                            ids,
                        ));
                    }
                    Inline::Strong { content }
                    | Inline::Emphasis { content }
                    | Inline::Strikethrough { content } => inlines(content, output),
                    _ => {}
                }
            }
        }
        fn blocks(values: &[Block], output: &mut Vec<(String, String, Vec<String>)>) {
            for block in values {
                match block {
                    Block::Paragraph { content } | Block::Heading { content, .. } => {
                        inlines(content, output)
                    }
                    Block::Blockquote { children, .. } => blocks(children, output),
                    Block::Custom(node) => blocks(&node.children, output),
                    Block::Table(table) => {
                        for cell in table.head.iter().chain(table.rows.iter().flatten()) {
                            inlines(&cell.content, output);
                        }
                    }
                    _ => {}
                }
            }
        }
        let mut found = Vec::new();
        for row in &self.rows {
            blocks(&row.blocks, &mut found);
        }
        found
            .into_iter()
            .filter_map(|(target, label, ids)| {
                if !super::super::links::allowed_link(&target) || ids.is_empty() {
                    return None;
                }
                let fragments = ids
                    .iter()
                    .map(|id| self.fragments.iter().find(|fragment| &fragment.id == id))
                    .collect::<Option<Vec<_>>>()?;
                let first = fragments.first()?;
                let last = fragments.last()?;
                Some((
                    super::super::links::LinkRange {
                        range: first.range.start..last.range.end,
                        target,
                        occurrence: 0,
                        label,
                        owner: first.id.clone(),
                        starts_here: true,
                        text_identity: 0,
                    },
                    ids,
                ))
            })
            .collect()
    }

    fn matches_snapshot(&self, snapshot: &ReadingSnapshot) -> bool {
        let mut expected = self
            .fragments
            .iter()
            .filter(|fragment| !fragment.text.is_empty());
        for text in &snapshot.texts {
            for tag in &text.content.test_ids {
                let Some(fragment) = expected.next() else {
                    return false;
                };
                if fragment.id != tag.test_id.as_ref() {
                    return false;
                }
                let Some(start) = utf16_byte(&text.content.text, tag.start) else {
                    return false;
                };
                let Some(end) = utf16_byte(&text.content.text, tag.end) else {
                    return false;
                };
                if text.content.text.get(start..end) != Some(fragment.text.as_str()) {
                    return false;
                }
            }
        }
        expected.next().is_none()
    }

    fn display_endpoint(
        &self,
        snapshot: &ReadingSnapshot,
        endpoint: (usize, u32),
    ) -> Option<usize> {
        let mounted = snapshot.texts.get(endpoint.0)?;
        let byte = utf16_byte(&mounted.content.text, endpoint.1)?;
        mounted.content.test_ids.iter().find_map(|tag| {
            let fragment = self
                .fragments
                .iter()
                .find(|fragment| fragment.id == tag.test_id.as_ref())?;
            let start = utf16_byte(&mounted.content.text, tag.start)?;
            let end = utf16_byte(&mounted.content.text, tag.end)?;
            (byte >= start
                && byte <= end
                && mounted.content.text.get(start..end) == Some(fragment.text.as_str()))
            .then(|| fragment.range.start + byte - start)
        })
    }

    fn native_endpoint(&self, snapshot: &ReadingSnapshot, byte: usize) -> Option<(usize, u32)> {
        for (index, mounted) in snapshot.texts.iter().enumerate() {
            for tag in &mounted.content.test_ids {
                let Some(fragment) = self
                    .fragments
                    .iter()
                    .find(|fragment| fragment.id == tag.test_id.as_ref())
                else {
                    continue;
                };
                if byte < fragment.range.start || byte > fragment.range.end {
                    continue;
                }
                let start = utf16_byte(&mounted.content.text, tag.start)?;
                let end = utf16_byte(&mounted.content.text, tag.end)?;
                if mounted.content.text.get(start..end) != Some(fragment.text.as_str()) {
                    return None;
                }
                let local = byte - fragment.range.start;
                let prefix = fragment.text.get(..local)?;
                let units = u32::try_from(prefix.encode_utf16().count()).ok()?;
                return Some((index, tag.start.checked_add(units)?));
            }
        }
        None
    }

    fn selected(&self, snapshot: &ReadingSnapshot, source: &str) -> Option<(Range<usize>, String)> {
        let mapping = self.mapping(source);
        let mut selected: Option<Range<usize>> = None;
        for (index, utf16) in &snapshot.ranges {
            if utf16.is_empty() {
                continue;
            }
            let mounted = snapshot.texts.get(*index)?;
            let start = utf16_byte(&mounted.content.text, utf16.start)?;
            let end = utf16_byte(&mounted.content.text, utf16.end)?;
            let mut cursor = start;
            for tag in &mounted.content.test_ids {
                let Some(fragment) = self
                    .fragments
                    .iter()
                    .find(|fragment| fragment.id == tag.test_id.as_ref())
                else {
                    continue;
                };
                let tag_start = utf16_byte(&mounted.content.text, tag.start)?;
                let tag_end = utf16_byte(&mounted.content.text, tag.end)?;
                if mounted.content.text.get(tag_start..tag_end)? != fragment.text {
                    return None;
                }
                let from = start.max(tag_start);
                let to = end.min(tag_end);
                if from >= to {
                    continue;
                }
                if from != cursor {
                    return None;
                }
                let range =
                    fragment.range.start + from - tag_start..fragment.range.start + to - tag_start;
                if !fragment.synthetic {
                    let mut covered = range.start;
                    for span in &mapping {
                        if span.rendered.start <= covered && span.rendered.end > covered {
                            covered = span.rendered.end.min(range.end);
                        }
                        if covered == range.end {
                            break;
                        }
                    }
                    if covered != range.end
                        && (!fragment.code || !self.code_range_mapped(fragment, &mapping))
                    {
                        return None;
                    }
                }
                if let Some(previous) = &mut selected {
                    if range.start < previous.end {
                        return None;
                    }
                    previous.end = range.end;
                } else {
                    selected = Some(range);
                }
                cursor = to;
            }
            if cursor != end {
                return None;
            }
        }
        let range = selected?;
        Some((range.clone(), self.text.get(range)?.to_owned()))
    }
}

fn utf16_byte(text: &str, target: u32) -> Option<usize> {
    let mut units = 0;
    for (byte, ch) in text.char_indices() {
        if units == target {
            return Some(byte);
        }
        units += ch.len_utf16() as u32;
        if units > target {
            return None;
        }
    }
    (units == target).then_some(text.len())
}

#[derive(Clone)]
struct Projection {
    session: ProjectionId,
    revision: u64,
    source: String,
    content: Content,
    colors: vega_theme::ThemeColors,
    thinking: bool,
    scroll_ticket: ProjectionId,
}

enum Request {
    Layout(ProjectionId, f32, f32, u32),
    ScrollBinding(ProjectionId, u32, u32),
    Scroll(ProjectionId, u32, u32, ProjectionId, bool),
    Link(ProjectionId, String, String),
    Code(ProjectionId, String),
    CodeBounds(ProjectionId, String, [f32; 4]),
}

#[derive(Clone)]
pub(crate) struct ReadingBinding {
    state: WeakEntity<BodyState>,
    snapshot: Rc<RefCell<Option<ReadingSnapshot>>>,
    view: WeakEntity<nsh_platform_gpui::AppView>,
    observation: Rc<Cell<u64>>,
}

#[derive(Clone)]
pub(crate) struct RetainedSelection {
    pub(crate) source_endpoints:
        Option<(vega_markdown::SourcePosition, vega_markdown::SourcePosition)>,
}

impl ReadingBinding {
    fn observe_session(
        &self,
        session: ProjectionId,
        snapshot: Option<ReadingSnapshot>,
        window: &mut Window,
        cx: &mut App,
    ) {
        if !self
            .state
            .upgrade()
            .is_some_and(|state| state.read(cx).projection.session.matches(&session))
        {
            return;
        }
        let serial = self.observation.get().wrapping_add(1);
        self.observation.set(serial);
        let observation = self.observation.clone();
        let state = self.state.clone();
        window.defer(cx, move |window, cx| {
            if observation.get() != serial {
                return;
            }
            let _ = state.update(cx, |state, cx| {
                if !state.projection.session.matches(&session) {
                    return;
                }
                if let Some(snapshot) = snapshot {
                    if !state.sync_selection(
                        &ReadingRequest {
                            intent: ReadingIntent::ContextMenu,
                            snapshot,
                        },
                        window,
                        cx,
                    ) {
                        state.copy.invalidate_selection();
                        state.snapshot.borrow_mut().take();
                    }
                } else {
                    let current =
                        state
                            .surface
                            .as_ref()
                            .zip(state.area)
                            .and_then(|(surface, area)| {
                                surface.entity().read(cx).reading_snapshot(area)
                            });
                    if current.as_ref().is_some_and(|snapshot| {
                        snapshot.ranges.iter().any(|(_, range)| !range.is_empty())
                    }) {
                        return;
                    }
                    state.copy.invalidate_selection();
                    state.snapshot.borrow_mut().take();
                    state.retained.borrow_mut().take();
                }
            });
        });
    }

    pub(crate) fn selection_current(&self, cx: &App) -> bool {
        self.snapshot.borrow().as_ref().is_some_and(|snapshot| {
            self.view
                .upgrade()
                .is_some_and(|view| view.read(cx).reading_snapshot_is_current(snapshot))
        })
    }

    pub(crate) fn copy(&self, window: &mut Window, cx: &mut App) {
        let Some(state) = self.state.upgrade() else {
            return;
        };
        let current = state.read(cx);
        let session = current.projection.session.clone();
        let Some(snapshot) = current
            .surface
            .as_ref()
            .zip(current.area)
            .and_then(|(surface, area)| surface.entity().read(cx).reading_snapshot(area))
        else {
            return;
        };
        let state = state.downgrade();
        window.defer(cx, move |window, cx| {
            let _ = state.update(cx, |state, cx| {
                if state.projection.session.matches(&session)
                    && state.sync_selection(
                        &ReadingRequest {
                            intent: ReadingIntent::Copy,
                            snapshot,
                        },
                        window,
                        cx,
                    )
                {
                    state.copy.copy_selected_text(cx);
                }
            });
        });
    }

    pub(crate) fn refresh(&self, window: &Window, cx: &mut App) -> bool {
        let Some(captured) = self.snapshot.borrow().clone() else {
            return false;
        };
        let Some(snapshot) = self
            .view
            .upgrade()
            .and_then(|view| view.read(cx).reading_snapshot(captured.area))
        else {
            return false;
        };
        self.state
            .update(cx, |state, cx| {
                state.sync_selection(
                    &ReadingRequest {
                        intent: ReadingIntent::ContextMenu,
                        snapshot,
                    },
                    window,
                    cx,
                )
            })
            .unwrap_or(false)
    }

    pub(super) fn attach_menu(&self, menu: WeakEntity<super::MenuState>, cx: &mut App) {
        let _ = self.state.update(cx, |state, _| state.menu = Some(menu));
    }
}

struct CodeTarget {
    binding: String,
    bounds: [f32; 4],
    focus: FocusHandle,
    snapshot: ReadingSnapshot,
}

struct BodyState {
    copy: MessageCopy,
    owner: Option<ReadingOwner>,
    projection: Projection,
    surface: Option<Surface<Projection, Request>>,
    reading: Rc<RefCell<Vec<ReadingRequest>>>,
    snapshot: Rc<RefCell<Option<ReadingSnapshot>>>,
    last_geometry: Option<Geometry>,
    acknowledged: Option<ProjectionId>,
    area: Option<u32>,
    retained: Rc<RefCell<Option<RetainedSelection>>>,
    lease: Weak<Lease>,
    height: f32,
    menu: Option<WeakEntity<super::MenuState>>,
    code_targets: Vec<CodeTarget>,
    pending_remap: Option<RetainedSelection>,
    listener_session: Option<ProjectionId>,
    observation: Rc<Cell<u64>>,
    opacity: opacity::Publication,
    scroll_node: Option<u32>,
    follow: bool,
    source_anchor: Option<source_anchor::Saved>,
    source_mount: Option<Rc<Cell<bool>>>,
    scroll_intent: u64,
    anchor_scroll: Option<ProjectionId>,
    scroll_content_height: f32,
    source_layout_width: Option<f32>,
}

impl BodyState {
    fn interrupt_anchor(&mut self) {
        self.scroll_intent = self.scroll_intent.wrapping_add(1);
        self.source_anchor = None;
        self.anchor_scroll = None;
        self.follow = false;
        self.projection.scroll_ticket = ProjectionId::new();
        if let Some(surface) = &self.surface {
            surface.publish(self.projection.clone());
        }
    }

    fn observe_scroll(
        &mut self,
        session: &ProjectionId,
        area: u32,
        scroll_node: u32,
        ticket: &ProjectionId,
        follow: bool,
    ) -> bool {
        if !self.projection.session.matches(session)
            || !self.projection.scroll_ticket.matches(ticket)
            || self.area != Some(area)
            || self.scroll_node != Some(scroll_node)
        {
            return false;
        }
        let restored = self
            .anchor_scroll
            .as_ref()
            .is_some_and(|anchor| anchor.matches(ticket));
        let saved = self
            .source_anchor
            .take()
            .filter(|saved| restored && saved.current(self));
        self.scroll_intent = self.scroll_intent.wrapping_add(1);
        if restored {
            self.anchor_scroll = None;
            self.source_anchor = saved.map(|mut saved| {
                saved.intent = self.scroll_intent;
                saved
            });
        } else {
            self.follow = follow;
        }
        true
    }

    fn apply_source_anchor(&mut self, window: &Window, cx: &mut App) {
        if !self.projection.thinking
            || !self
                .owner
                .as_ref()
                .is_some_and(|owner| owner.current_for_frame(self.copy.id, window, cx))
            || self.copy.content_generation() != self.projection.revision
            || self.copy.display_text() != self.projection.source
        {
            self.source_anchor = None;
            return;
        }
        if !self.valid(window, cx) {
            if !(self.acknowledged.is_none()
                && self.source_layout_width.is_none()
                && self
                    .source_anchor
                    .as_ref()
                    .is_some_and(|saved| saved.pending && saved.current(self))
                && self.projection_current(window, cx))
            {
                self.source_anchor = None;
            }
            return;
        }
        let Some(mount) = self.source_mount.clone().filter(|mount| mount.get()) else {
            self.source_anchor = None;
            return;
        };
        let Some(host) = self.lease.upgrade().and_then(|lease| lease.geometry.get()) else {
            self.source_anchor = None;
            return;
        };
        let Some((surface, area)) = self.surface.as_ref().zip(self.area) else {
            self.source_anchor = None;
            return;
        };
        let Some(scroll_node) = self.scroll_node else {
            self.source_anchor = None;
            return;
        };
        if self.source_layout_width != Some(f32::from(host.bounds.size.width)) {
            return;
        }
        let view = surface.entity();
        if self.source_anchor.as_ref().is_some_and(|saved| {
            !saved.pending
                && saved.current(self)
                && saved.host == host
                && saved.area == area
                && view.read(cx).reading_geometry_is_current(&saved.geometry)
        }) {
            return;
        }
        let Some(snapshot) = view.read(cx).reading_snapshot(area) else {
            self.source_anchor = None;
            return;
        };
        if !self.projection.content.matches_snapshot(&snapshot) {
            return;
        }
        if snapshot.ranges.iter().any(|(_, range)| !range.is_empty()) {
            self.source_anchor = None;
            return;
        }
        let Some(geometry) = view.read(cx).reading_geometry(&snapshot) else {
            self.source_anchor = None;
            return;
        };
        let clipped = host.bounds.intersect(&host.clip).intersect(&host.list);
        let viewport = [
            f32::from(clipped.origin.x - host.bounds.origin.x),
            f32::from(clipped.origin.y - host.bounds.origin.y),
            f32::from(clipped.size.width),
            f32::from(clipped.size.height),
        ];
        if clipped.is_empty() {
            self.source_anchor = None;
            return;
        }
        let saved = self
            .source_anchor
            .take()
            .filter(|saved| saved.current(self));
        if self.follow {
            let delta = self.scroll_content_height.max(0.);
            if delta.is_finite()
                && self.valid(window, cx)
                && view
                    .read(cx)
                    .scroll_reading_anchor(&geometry, scroll_node, delta)
            {
                self.anchor_scroll = Some(self.projection.scroll_ticket.clone());
            }
            return;
        }
        if let Some(mut saved) = saved {
            if !saved.pending && saved.host.bounds.size == host.bounds.size {
                saved.host = host;
                saved.geometry = geometry;
                saved.area = area;
                self.source_anchor = Some(saved);
                return;
            }
            if let Some(delta) = saved.anchor.delta(
                &self.projection.content,
                &self.projection.source,
                &snapshot,
                &geometry,
                viewport[1],
            ) && saved.current(self)
                && self.valid(window, cx)
                && self.lease.upgrade().and_then(|lease| lease.geometry.get()) == Some(host)
            {
                let applied = view
                    .read(cx)
                    .scroll_reading_anchor(&geometry, scroll_node, delta);
                if applied {
                    self.anchor_scroll = Some(self.projection.scroll_ticket.clone());
                    saved.pending = false;
                    saved.host = host;
                    saved.geometry = geometry;
                    saved.area = area;
                    self.source_anchor = Some(saved);
                }
            }
            return;
        }
        self.source_anchor = source_anchor::Anchor::capture(
            &self.projection.content,
            &self.projection.source,
            &snapshot,
            &geometry,
            viewport,
        )
        .map(|anchor| source_anchor::Saved {
            anchor,
            session: self.projection.session.clone(),
            pending: false,
            host,
            geometry,
            area,
            mount,
            intent: self.scroll_intent,
            observation: self.observation.get(),
        });
    }

    fn restore_selection(&mut self, window: &Window, cx: &mut App) {
        if !self.valid(window, cx) {
            return;
        }
        let Some(retained) = self.pending_remap.take() else {
            return;
        };
        let Some((anchor, focus)) = retained.source_endpoints else {
            return;
        };
        let Some(surface) = &self.surface else {
            return;
        };
        let Some(expected) = self
            .area
            .and_then(|area| surface.entity().read(cx).reading_snapshot(area))
        else {
            return;
        };
        if !self.projection.content.matches_snapshot(&expected) {
            return;
        }
        let mapping = self.projection.content.mapping(&self.copy.display_text());
        let endpoint = |position: &vega_markdown::SourcePosition| {
            let byte = mapping
                .iter()
                .find_map(|span| span.rendered_position(position))?;
            self.projection.content.native_endpoint(&expected, byte)
        };
        let Some(anchor) = endpoint(&anchor) else {
            return;
        };
        let Some(focus) = endpoint(&focus) else {
            return;
        };
        let installed = surface
            .entity()
            .read(cx)
            .install_reading_selection(&expected, anchor, focus);
        if let Some(snapshot) = installed {
            self.sync_selection(
                &ReadingRequest {
                    intent: ReadingIntent::ContextMenu,
                    snapshot,
                },
                window,
                cx,
            );
        }
    }

    fn install_listener(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self
            .listener_session
            .as_ref()
            .is_some_and(|session| session.matches(&self.projection.session))
        {
            return;
        }
        let Some(surface) = &self.surface else {
            return;
        };
        let Some(area) = self.area else {
            return;
        };
        let binding = ReadingBinding {
            state: cx.weak_entity(),
            snapshot: self.snapshot.clone(),
            view: surface.entity().downgrade(),
            observation: self.observation.clone(),
        };
        let session = self.projection.session.clone();
        let observer_session = session.clone();
        let app = cx.to_async();
        let handle = window.window_handle();
        let initializing = Rc::new(Cell::new(true));
        let listener_initializing = initializing.clone();
        let installed = surface.entity().read(cx).set_reading_selection_listener(
            area,
            Some(Rc::new(move |snapshot| {
                if listener_initializing.get() && snapshot.is_none() {
                    return;
                }
                let binding = binding.clone();
                let session = observer_session.clone();
                let mut app = app.clone();
                let executor = app.foreground_executor().clone();
                executor
                    .spawn(async move {
                        let _ = app.update_window(handle, |_, window, cx| {
                            binding.observe_session(session, snapshot, window, cx);
                        });
                    })
                    .detach();
            })),
        );
        initializing.set(false);
        if installed {
            self.listener_session = Some(session);
        }
    }

    fn copy_code(&self, session: &ProjectionId, binding: &str, window: &Window, cx: &mut App) {
        if !self.projection.session.matches(session)
            || !self.valid(window, cx)
            || !self.mounted_content_current(cx)
        {
            return;
        }
        if let Some(fragment) = self
            .projection
            .content
            .fragments
            .iter()
            .find(|fragment| fragment.id == binding && fragment.code)
            && self.projection.content.code_range_mapped(
                fragment,
                &self.projection.content.mapping(&self.copy.display_text()),
            )
        {
            cx.write_to_clipboard(ClipboardItem::new_string(fragment.text.clone()));
        }
    }

    fn copy_code_target(
        &self,
        session: &ProjectionId,
        binding: &str,
        snapshot: &ReadingSnapshot,
        geometry: Geometry,
        window: &Window,
        cx: &mut App,
    ) {
        if self.lease.upgrade().and_then(|lease| lease.geometry.get()) != Some(geometry)
            || !self.surface.as_ref().is_some_and(|surface| {
                surface
                    .entity()
                    .read(cx)
                    .reading_snapshot_is_current(snapshot)
            })
        {
            return;
        }
        self.copy_code(session, binding, window, cx);
    }

    fn valid(&self, window: &Window, cx: &App) -> bool {
        self.acknowledged
            .as_ref()
            .is_some_and(|session| session.matches(&self.projection.session))
            && self.projection_current(window, cx)
    }

    fn projection_current(&self, window: &Window, cx: &App) -> bool {
        self.owner
            .as_ref()
            .is_some_and(|owner| owner.current_for_frame(self.copy.id, window, cx))
            && self.copy.content_generation() == self.projection.revision
            && self.copy.display_text() == self.projection.source
            && self
                .surface
                .as_ref()
                .is_some_and(|surface| surface.errors().borrow().is_none())
            && self.lease.upgrade().is_some_and(|lease| {
                lease.live.get()
                    && self.owner.as_ref().is_some_and(|owner| {
                        super::mounted::lease_current(&lease, &owner.stream, window, cx)
                    })
                    && lease.geometry.get().is_some_and(|geometry| {
                        geometry.viewport == window.viewport_size()
                            && geometry.scale == window.scale_factor()
                            && !geometry.bounds.intersect(&geometry.clip).is_empty()
                            && self
                                .owner
                                .as_ref()
                                .and_then(|owner| {
                                    owner.stream.upgrade().map(|stream| (owner, stream))
                                })
                                .is_some_and(|(owner, stream)| {
                                    let stream = stream.read(cx);
                                    geometry.list
                                        == super::mounted::list_bounds(&owner.stream, window, cx)
                                            .unwrap_or_else(|| stream.list.viewport_bounds())
                                        && geometry.foreground == stream.foreground_height
                                })
                    })
            })
    }

    fn mounted_content_current(&self, cx: &App) -> bool {
        self.surface
            .as_ref()
            .zip(self.area)
            .and_then(|(surface, area)| surface.entity().read(cx).reading_snapshot(area))
            .is_some_and(|snapshot| self.projection.content.matches_snapshot(&snapshot))
    }

    fn sync_selection(&self, request: &ReadingRequest, window: &Window, cx: &mut App) -> bool {
        if !self.valid(window, cx)
            || !self.surface.as_ref().is_some_and(|surface| {
                surface
                    .entity()
                    .read(cx)
                    .reading_snapshot_is_current(&request.snapshot)
            })
        {
            return false;
        }
        *self.snapshot.borrow_mut() = Some(request.snapshot.clone());
        if request
            .snapshot
            .ranges
            .iter()
            .all(|(_, range)| range.is_empty())
        {
            self.copy.invalidate_selection();
            self.retained.borrow_mut().take();
            return true;
        }
        let Some((range, value)) = self
            .projection
            .content
            .selected(&request.snapshot, &self.copy.display_text())
        else {
            self.copy.invalidate_selection();
            self.retained.borrow_mut().take();
            return false;
        };
        self.copy
            .set_visible_text(self.projection.content.text.clone());
        let spans = self.projection.content.mapping(&self.copy.display_text());
        let source_position = |byte| {
            spans
                .iter()
                .find(|span| span.rendered.contains(&byte))
                .or_else(|| spans.iter().rev().find(|span| span.rendered.end == byte))
                .map(|span| span.position(byte))
        };
        let source_endpoints = request.snapshot.endpoints.and_then(|(anchor, focus)| {
            Some((
                source_position(
                    self.projection
                        .content
                        .display_endpoint(&request.snapshot, anchor)?,
                )?,
                source_position(
                    self.projection
                        .content
                        .display_endpoint(&request.snapshot, focus)?,
                )?,
            ))
        });
        *self.retained.borrow_mut() = Some(RetainedSelection { source_endpoints });
        self.copy.set_local_selection_range(range.clone());
        self.copy.set_projected_selection(Some(range));
        self.copy
            .set_selected_text(self.copy.selection_generation(), value);
        *self.snapshot.borrow_mut() = Some(request.snapshot.clone());
        true
    }

    fn drain_reading(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let geometry = self.lease.upgrade().and_then(|lease| lease.geometry.get());
        if self.last_geometry.is_some() && self.last_geometry != geometry {
            self.copy.invalidate_selection();
            self.snapshot.borrow_mut().take();
            self.reading.borrow_mut().clear();
        }
        self.last_geometry = geometry;
        let requests = std::mem::take(&mut *self.reading.borrow_mut());
        for request in requests {
            if !self.sync_selection(&request, window, cx) {
                continue;
            }
            match request.intent {
                ReadingIntent::Copy => self.copy.copy_selected_text(cx),
                ReadingIntent::PrimarySelection => {
                    #[cfg(target_os = "linux")]
                    if let Some(text) = self.copy.selected_text_snapshot() {
                        cx.write_to_primary(ClipboardItem::new_string(text));
                    }
                }
                ReadingIntent::ContextMenu => {
                    if let Some(menu) = self.menu.clone() {
                        let position = window.mouse_position();
                        window.defer(cx, move |window, cx| {
                            let _ = menu.update(cx, |menu, cx| menu.open(position, window, cx));
                        });
                    }
                }
            }
        }
    }
}

pub(crate) fn render_model(
    model: &super::super::model::StreamModel,
    copy: &MessageCopy,
    colors: vega_theme::ThemeColors,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let cache = window.use_keyed_state(("nsh-reading-content", copy.id), cx, |_, _| {
        None::<(u64, u64, Content)>
    });
    let content = cache.update(cx, |cache, _| {
        let revision = model.content_revision();
        let generation = copy.content_generation();
        if !cache
            .as_ref()
            .is_some_and(|(old_revision, old_generation, _)| {
                *old_revision == revision && *old_generation == generation
            })
        {
            *cache = Some((revision, generation, Content::from_model(model, copy.id)));
        }
        cache
            .as_ref()
            .map(|(_, _, content)| content.clone())
            .unwrap_or_default()
    });
    render(content, copy, false, colors, window, cx)
}

pub(crate) fn render(
    content: Content,
    copy: &MessageCopy,
    thinking: bool,
    colors: vega_theme::ThemeColors,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    render_fading(
        content,
        copy,
        thinking,
        colors,
        copy.stream_fade.clone(),
        window,
        cx,
    )
}

pub(in crate::conversation_stream) fn render_fading(
    content: Content,
    copy: &MessageCopy,
    thinking: bool,
    colors: vega_theme::ThemeColors,
    fade: Option<opacity::FadeSource>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let id = copy.id;
    let state = window.use_keyed_state(("nsh-reading-body", id), cx, |_, _| BodyState {
        copy: copy.clone(),
        owner: None,
        projection: Projection {
            session: ProjectionId::new(),
            revision: copy.content_generation(),
            source: copy.display_text(),
            content: content.clone(),
            colors,
            thinking,
            scroll_ticket: ProjectionId::new(),
        },
        surface: None,
        reading: Rc::new(RefCell::new(Vec::new())),
        snapshot: Default::default(),
        last_geometry: None,
        acknowledged: None,
        area: None,
        retained: Default::default(),
        lease: Weak::new(),
        height: 28.,
        menu: None,
        code_targets: Vec::new(),
        pending_remap: None,
        listener_session: None,
        observation: Rc::new(Cell::new(0)),
        opacity: opacity::Publication::new(),
        scroll_node: None,
        follow: false,
        source_anchor: None,
        source_mount: None,
        scroll_intent: 0,
        anchor_scroll: None,
        scroll_content_height: 0.,
        source_layout_width: None,
    });
    let lease = state.read(cx).lease.upgrade().unwrap_or_else(|| {
        Rc::new(Lease {
            frame: Default::default(),
            live: Cell::new(false),
            geometry: Cell::new(None),
        })
    });
    state.update(cx, |this, cx| {
        let owner = copy.reading_owner.borrow().clone();
        let rebound = match (this.owner.as_ref(), owner.as_ref()) {
            (Some(previous), Some(current)) => !previous.same_binding(current),
            (None, None) => false,
            _ => true,
        };
        let changed = this.projection.revision != copy.content_generation()
            || this.projection.source != copy.display_text()
            || this.projection.content.source_start != content.source_start
            || this.projection.content.source_prefix != content.source_prefix
            || this.projection.content.source_domain != content.source_domain
            || this.projection.content.text != content.text
            || this.projection.content.document != content.document
            || rebound;
        if rebound {
            this.interrupt_anchor();
        }
        if changed {
            this.source_anchor = this.source_anchor.take().and_then(|mut saved| {
                if rebound || !saved.current(this) {
                    return None;
                }
                saved.pending = true;
                saved.anchor = saved.anchor.remap(
                    &content,
                    &this.projection.content,
                    &copy.display_text(),
                    &this.projection.source,
                )?;
                Some(saved)
            });
        }
        this.opacity.set_source(fade.clone());
        this.copy = copy.clone();
        this.owner = owner;
        this.lease = Rc::downgrade(&lease);
        if changed {
            this.clear_opacity(cx);
            this.pending_remap = if !rebound
                && content.source_continues(
                    &this.projection.content,
                    &copy.display_text(),
                    &this.projection.source,
                ) {
                this.retained
                    .borrow()
                    .clone()
                    .or_else(|| this.pending_remap.take())
            } else {
                None
            };
            this.listener_session = None;
            this.retained.borrow_mut().take();
            this.copy.invalidate_selection();
            this.snapshot.borrow_mut().take();
            this.acknowledged = None;
            this.source_layout_width = None;
            this.code_targets.clear();
            this.anchor_scroll = None;
            this.projection = Projection {
                session: ProjectionId::new(),
                revision: copy.content_generation(),
                source: copy.display_text(),
                content,
                colors,
                thinking,
                scroll_ticket: ProjectionId::new(),
            };
        }
        if let Some(saved) = &mut this.source_anchor {
            saved.session = this.projection.session.clone();
        }
        if this.surface.is_none() {
            let (surface, mut requests) =
                Surface::new_transparent(this.projection.clone(), pixels, window, cx);
            let queue = this.reading.clone();
            let snapshot = this.snapshot.clone();
            let weak = cx.weak_entity();
            let app = cx.to_async();
            let handle = window.window_handle();
            surface
                .entity()
                .read(cx)
                .set_reading_handler(Some(Rc::new(move |request| {
                    *snapshot.borrow_mut() = Some(request.snapshot.clone());
                    queue.borrow_mut().push(request);
                    let weak = weak.clone();
                    let mut app = app.clone();
                    let executor = app.foreground_executor().clone();
                    executor
                        .spawn(async move {
                            let _ = app.update_window(handle, |_, window, cx| {
                                let _ =
                                    weak.update(cx, |state, cx| state.drain_reading(window, cx));
                            });
                        })
                        .detach();
                })));
            *copy.nsh_reading.borrow_mut() = Some(ReadingBinding {
                state: cx.weak_entity(),
                snapshot: this.snapshot.clone(),
                view: surface.entity().downgrade(),
                observation: this.observation.clone(),
            });
            cx.spawn(async move |this, cx| {
                while let Some(request) = requests.recv().await {
                    if this
                        .update_in(cx, |this, window, cx| match request {
                            Request::Layout(session, width, height, area) => {
                                if this.projection.session.matches(&session)
                                    && width.is_finite()
                                    && width > 0.
                                    && height.is_finite()
                                    && height >= 0.
                                {
                                    if this.source_layout_width != Some(width) {
                                        this.source_layout_width = Some(width);
                                        window.refresh();
                                    }
                                    this.scroll_content_height = height;
                                    this.acknowledged = Some(session);
                                    this.area = Some(area);
                                    this.install_listener(window, cx);
                                    let state = cx.weak_entity();
                                    let session = this.projection.session.clone();
                                    window.defer(cx, move |window, cx| {
                                        let _ = state.update(cx, |state, cx| {
                                            if state.projection.session.matches(&session) {
                                                state.restore_selection(window, cx);
                                            }
                                        });
                                    });
                                    let height = if this.projection.thinking {
                                        height
                                            .min(vega_theme::Layout::DISCLOSURE_CONTENT_MAX_HEIGHT)
                                    } else {
                                        height
                                    };
                                    if (height - this.height).abs() > 0.5 {
                                        this.height = height.max(1.);
                                        window.refresh();
                                    }
                                }
                            }
                            Request::ScrollBinding(session, area, scroll_node) => {
                                if this.projection.session.matches(&session)
                                    && this.area == Some(area)
                                    && this.scroll_node != Some(scroll_node)
                                {
                                    this.interrupt_anchor();
                                    this.scroll_node = Some(scroll_node);
                                    window.refresh();
                                }
                            }
                            Request::Scroll(session, area, scroll_node, ticket, follow) => {
                                if this.observe_scroll(&session, area, scroll_node, &ticket, follow)
                                {
                                    window.refresh();
                                }
                            }
                            Request::Link(session, url, binding) => {
                                if this.projection.session.matches(&session)
                                    && this.valid(window, cx)
                                    && this.mounted_content_current(cx)
                                    && this.projection.content.reading_links().iter().any(
                                        |(link, ids)| link.target == url && ids.contains(&binding),
                                    )
                                {
                                    cx.open_url(&url);
                                }
                            }
                            Request::Code(session, binding) => {
                                this.copy_code(&session, &binding, window, cx);
                            }
                            Request::CodeBounds(session, binding, bounds) => {
                                if this.projection.session.matches(&session)
                                    && bounds.iter().all(|value| value.is_finite())
                                    && bounds[2] > 0.
                                    && bounds[3] > 0.
                                    && let Some(snapshot) =
                                        this.surface.as_ref().zip(this.area).and_then(
                                            |(surface, area)| {
                                                surface.entity().read(cx).reading_snapshot(area)
                                            },
                                        )
                                {
                                    if let Some(target) = this
                                        .code_targets
                                        .iter_mut()
                                        .find(|target| target.binding == binding)
                                    {
                                        target.bounds = bounds;
                                        target.snapshot = snapshot;
                                    } else {
                                        this.code_targets.push(CodeTarget {
                                            binding,
                                            bounds,
                                            focus: cx.focus_handle().tab_stop(true),
                                            snapshot,
                                        });
                                    }
                                    window.refresh();
                                }
                            }
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();
            this.surface = Some(surface);
            cx.on_release(|this, cx| {
                this.clear_opacity(cx);
                if let Some(surface) = &this.surface {
                    surface.entity().read(cx).stop();
                }
            })
            .detach();
        } else if changed && let Some(surface) = &this.surface {
            surface.publish(this.projection.clone());
        }
        if let Some(surface) = &mut this.surface {
            surface.sync_environment(cx);
        }
    });
    let weak = state.downgrade();
    let (surface, height) = {
        let state = state.read(cx);
        (state.surface.as_ref().map(Surface::entity), state.height)
    };
    let mut child = div()
        .w_full()
        .h(px(height))
        .relative()
        .children(surface)
        .on_mouse_up(MouseButton::Left, {
            let weak = weak.clone();
            move |_, window, cx| {
                let _ = weak.update(cx, |this, cx| this.drain_reading(window, cx));
            }
        })
        .on_mouse_down(MouseButton::Right, {
            let weak = weak.clone();
            move |_, window, cx| {
                let _ = weak.update(cx, |this, cx| this.drain_reading(window, cx));
            }
        });
    {
        let state = state.read(cx);
        if let Some(geometry) = lease.geometry.get() {
            for target in &state.code_targets {
                let bounds = target.bounds;
                let rect = Bounds::new(
                    geometry.bounds.origin + point(px(bounds[0]), px(bounds[1])),
                    size(px(bounds[2]), px(bounds[3])),
                );
                if rect.intersect(&geometry.clip) != rect
                    || bounds[1] < 0.
                    || bounds[1] + bounds[3] > height + 0.5
                {
                    continue;
                }
                let session = state.projection.session.clone();
                let binding = target.binding.clone();
                let focus = target.focus.clone();
                let action = weak.clone();
                let pointer = weak.clone();
                let pointer_session = session.clone();
                let pointer_binding = binding.clone();
                let pointer_snapshot = target.snapshot.clone();
                let action_snapshot = target.snapshot.clone();
                let accessible_snapshot = target.snapshot.clone();
                let accessible = weak.clone();
                let accessible_session = session.clone();
                let accessible_binding = binding.clone();
                child = child.child(
                    div()
                        .id(ElementId::Name(
                            format!("nsh-reading-copy-{}", target.binding).into(),
                        ))
                        .absolute()
                        .left(px(bounds[0]))
                        .top(px(bounds[1]))
                        .w(px(bounds[2]))
                        .h(px(bounds[3]))
                        .track_focus(&focus)
                        .tab_stop(true)
                        .role(Role::Button)
                        .aria_label(vega_i18n::t("ui.copy"))
                        .when(focus.is_focused(window), |child| {
                            child.border_1().border_color(colors.accent)
                        })
                        .on_click(move |_, window, cx| {
                            let _ = pointer.update(cx, |state, cx| {
                                state.copy_code_target(
                                    &pointer_session,
                                    &pointer_binding,
                                    &pointer_snapshot,
                                    geometry,
                                    window,
                                    cx,
                                )
                            });
                        })
                        .on_key_down(move |event: &KeyDownEvent, window, cx| {
                            let modifiers = event.keystroke.modifiers;
                            if focus.is_focused(window)
                                && !event.prefer_character_input
                                && !modifiers.control
                                && !modifiers.alt
                                && !modifiers.platform
                                && !modifiers.function
                                && event.keystroke.key == "tab"
                            {
                                if modifiers.shift {
                                    window.focus_prev(cx);
                                } else {
                                    window.focus_next(cx);
                                }
                                cx.stop_propagation();
                                return;
                            }
                            if focus.is_focused(window)
                                && !event.prefer_character_input
                                && !event.is_held
                                && !modifiers.control
                                && !modifiers.alt
                                && !modifiers.platform
                                && !modifiers.function
                                && !modifiers.shift
                                && matches!(event.keystroke.key.as_str(), "enter" | "space" | " ")
                            {
                                let _ = action.update(cx, |state, cx| {
                                    state.copy_code_target(
                                        &session,
                                        &binding,
                                        &action_snapshot,
                                        geometry,
                                        window,
                                        cx,
                                    );
                                });
                                cx.stop_propagation();
                            }
                        })
                        .on_a11y_action(AccessibleAction::Click, move |_, window, cx| {
                            let _ = accessible.update(cx, |state, cx| {
                                state.copy_code_target(
                                    &accessible_session,
                                    &accessible_binding,
                                    &accessible_snapshot,
                                    geometry,
                                    window,
                                    cx,
                                );
                            });
                        }),
                );
            }
        }
    }
    super::mounted::Mounted {
        child: LinkedBody {
            child: child.into_any_element(),
            state: weak,
        }
        .into_any_element(),
        lease,
        id: ("nsh-reading-mounted", id).into(),
    }
    .into_any_element()
}

struct LinkedBody {
    child: AnyElement,
    state: WeakEntity<BodyState>,
}

impl IntoElement for LinkedBody {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for LinkedBody {
    type RequestLayoutState = ();
    type PrepaintState = Vec<AnyElement>;
    fn id(&self) -> Option<ElementId> {
        Some(("nsh-reading-opacity", self.state.entity_id()).into())
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
        (self.child.request_layout(window, cx), ())
    }
    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Vec<AnyElement> {
        if let Some(entity) = self.state.upgrade() {
            let state = entity.read(cx);
            if let Some(lease) = state.lease.upgrade() {
                let geometry = state
                    .copy
                    .reading_owner
                    .borrow()
                    .as_ref()
                    .and_then(|owner| owner.stream.upgrade().map(|stream| (owner, stream)))
                    .map(|(owner, stream)| {
                        let stream = stream.read(cx);
                        Geometry {
                            bounds,
                            clip: window.content_mask().bounds,
                            viewport: window.viewport_size(),
                            scale: window.scale_factor(),
                            list: super::mounted::list_bounds(&owner.stream, window, cx)
                                .unwrap_or_else(|| stream.list.viewport_bounds()),
                            foreground: stream.foreground_height,
                        }
                    });
                lease.geometry.set(geometry);
            }
        }
        if let Some(id) = id {
            window.with_element_state(
                id,
                |presence: Option<(opacity::Presence, source_anchor::Presence)>, _| {
                    let presence = presence.unwrap_or_else(|| {
                        let live = Rc::new(Cell::new(true));
                        let _ = self.state.update(cx, |state, _| {
                            state.interrupt_anchor();
                            state.source_mount = Some(live.clone());
                        });
                        (
                            opacity::Presence::new(self.state.clone(), cx),
                            source_anchor::Presence(live),
                        )
                    });
                    ((), presence)
                },
            );
        }
        self.child.prepaint(window, cx);
        let _ = self
            .state
            .update(cx, |state, cx| state.apply_source_anchor(window, cx));
        let Some(entity) = self.state.upgrade() else {
            return Vec::new();
        };
        let state = entity.read(cx);
        if !state.valid(window, cx) {
            return Vec::new();
        }
        let links = state.projection.content.reading_links();
        if links.is_empty() {
            return Vec::new();
        }
        let Some(host) = state.lease.upgrade().and_then(|lease| lease.geometry.get()) else {
            return Vec::new();
        };
        let Some((surface, area)) = state.surface.as_ref().zip(state.area) else {
            return Vec::new();
        };
        let view = surface.entity();
        let Some(snapshot) = view.read(cx).reading_snapshot(area) else {
            return Vec::new();
        };
        if !state.projection.content.matches_snapshot(&snapshot) {
            return Vec::new();
        }
        let Some(geometry) = view.read(cx).reading_geometry(&snapshot) else {
            return Vec::new();
        };
        let session = state.projection.session.clone();
        let mut prepared = Vec::new();
        for (link, ids) in links {
            let mut rectangles = Vec::new();
            let mut complete = true;
            for id in &ids {
                let mut found = false;
                for (index, text) in snapshot.texts.iter().enumerate() {
                    for tag in text
                        .content
                        .test_ids
                        .iter()
                        .filter(|tag| tag.test_id.as_ref() == id)
                    {
                        found = true;
                        let Some(rects) = geometry.range_rects(index, tag.start..tag.end) else {
                            complete = false;
                            continue;
                        };
                        rectangles.extend(rects.into_iter().map(|rect| {
                            Bounds::new(
                                host.bounds.origin + point(px(rect[0]), px(rect[1])),
                                size(px(rect[2]), px(rect[3])),
                            )
                            .intersect(&host.clip)
                        }));
                    }
                }
                complete &= found;
            }
            if !complete || rectangles.iter().all(Bounds::is_empty) {
                continue;
            }
            let weak = self.state.clone();
            let session = session.clone();
            let link_identity = link.clone();
            let ids = ids.clone();
            let continuity: crate::link_preview::LinkValidity =
                Rc::new(move |window, cx| {
                    let Some(entity) = weak.upgrade() else {
                        return false;
                    };
                    let state = entity.read(cx);
                    state.projection.session.matches(&session)
                        && state.valid(window, cx)
                        && state.lease.upgrade().and_then(|lease| lease.geometry.get())
                            == Some(host)
                        && state.projection.content.reading_links().iter().any(
                            |(link, current_ids)| link == &link_identity && current_ids == &ids,
                        )
                });
            let source_current = continuity.clone();
            let weak = self.state.clone();
            let geometry = geometry.clone();
            let validity: crate::link_preview::LinkValidity = Rc::new(move |window, cx| {
                source_current(window, cx)
                    && weak.upgrade().is_some_and(|entity| {
                        entity.read(cx).surface.as_ref().is_some_and(|surface| {
                            surface
                                .entity()
                                .read(cx)
                                .reading_geometry_is_current(&geometry)
                        })
                    })
            });
            prepared.push((link, rectangles, validity));
        }
        let mut targets = Vec::new();
        for (link, rectangles, validity) in prepared {
            targets.extend(crate::link_preview::prepaint_measured_link_target(
                &link,
                rectangles,
                Vec::new(),
                Some(validity),
                window,
                cx,
            ));
        }
        targets
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        targets: &mut Vec<AnyElement>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let wheel = self.state.clone();
        window.on_mouse_event(move |_: &ScrollWheelEvent, phase, _, cx| {
            if phase.capture() {
                let _ = wheel.update(cx, |state, _| state.interrupt_anchor());
            }
        });
        let pointer = self.state.clone();
        window.on_mouse_event(move |_: &MouseDownEvent, phase, _, cx| {
            if phase.capture() {
                let _ = pointer.update(cx, |state, _| state.interrupt_anchor());
            }
        });
        let keyboard = self.state.clone();
        window.on_key_event(move |_: &KeyDownEvent, phase, _, cx| {
            if phase.capture() {
                let _ = keyboard.update(cx, |state, _| state.interrupt_anchor());
            }
        });
        let _ = self
            .state
            .update(cx, |state, cx| state.paint_opacity(window, cx));
        self.child.paint(window, cx);
        for target in targets {
            target.paint(window, cx);
        }
    }
}

fn pixels(
    incoming: nsh_core::prelude::Signal<Projection>,
    sender: tokio::sync::mpsc::Sender<Request>,
) -> nsh_core::prelude::AnyElement {
    use nsh_core::prelude::*;
    let commands = crate::nsh::OrderedCommands::new(sender);
    let owner = nsh_core::element::Owner::current();
    let scrolling = element_ref();
    let offset = signal(0.0f32);
    provide_context(ReadingScroll(offset));
    let content_commands = commands.clone();
    let content = each(
        move || incoming.with(|projection| projection.content.rows.clone()),
        |row| row.id,
        move |row, _| {
            let commands = content_commands.clone();
            view()
                .pt(move || px(row.with(|value| value.gap_before)))
                .child(dynamic(move || {
                    row.with(|value| native_row_blocks(&value.blocks, incoming, commands.clone()))
                }))
        },
    );
    let root = element_ref();
    let layout = commands.clone();
    let measurement = Rc::new(RefCell::new(None));
    effect(move || {
        let session = incoming.with(|projection| projection.session.clone());
        let layout = layout.clone();
        *measurement.borrow_mut() = Some(spawn(async move {
            if let Some(measured) = root.measure().await
                && let Some(area) = root.node()
            {
                layout.send(Request::Layout(
                    session.clone(),
                    measured.frame.size.width.0,
                    measured.frame.size.height.0,
                    area,
                ));
                if let Some(scroll_node) = scrolling.node() {
                    layout.send(Request::ScrollBinding(session, area, scroll_node));
                }
            }
        }));
    });
    let layout = commands.clone();
    let body = view()
        .element_ref(&root)
        .w_full()
        .flex_col()
        .selectable(true)
        .text_size(px(vega_theme::Typography::CONVERSATION))
        .text_color(move || {
            incoming.with(|projection| {
                crate::nsh::color(if projection.thinking {
                    projection.colors.text_secondary
                } else {
                    projection.colors.text_primary
                })
            })
        })
        .child(content)
        .on_layout(move |bounds: &Bounds<Points>| {
            let session = incoming.with(|projection| projection.session.clone());
            if let Some(area) = root.node() {
                owner.run(|| {
                    layout.send(Request::Layout(
                        session.clone(),
                        bounds.size.width.0,
                        bounds.size.height.0,
                        area,
                    ));
                    if let Some(scroll_node) = scrolling.node() {
                        layout.send(Request::ScrollBinding(session, area, scroll_node));
                    }
                });
            }
        });
    if incoming.get_untracked().thinking {
        let scroll_commands = commands.clone();
        let binding_commands = commands.clone();
        let scrolling_body = scroll()
            .element_ref(&scrolling)
            .w_full()
            .h_full()
            .scrollbar(true)
            .on_layout(move |_: &Bounds<Points>| {
                if let Some((area, scroll_node)) = root.node().zip(scrolling.node()) {
                    let session = incoming.with(|projection| projection.session.clone());
                    owner.run(|| {
                        binding_commands.send(Request::ScrollBinding(session, area, scroll_node))
                    });
                }
            })
            .on_scroll(move |event: &nsh_core::element::ScrollEvent| {
                offset.set(event.offset.y.0);
                if let Some((area, scroll_node)) = root.node().zip(scrolling.node()) {
                    let session = incoming.with(|projection| projection.session.clone());
                    let follow =
                        event.content_size.height.0 - event.size.height.0 - event.offset.y.0 <= 1.;
                    let ticket = incoming.with(|projection| projection.scroll_ticket.clone());
                    owner.run(|| {
                        scroll_commands.send(Request::Scroll(
                            session,
                            area,
                            scroll_node,
                            ticket,
                            follow,
                        ))
                    });
                }
            })
            .child(body);
        scrolling_body.into_element()
    } else {
        body.into_element()
    }
}

#[derive(Clone, Copy)]
struct ReadingScroll(nsh_core::prelude::Signal<f32>);

#[derive(Clone)]
struct TableWeights(std::collections::BTreeMap<String, String>);

fn code_icon(
    kind: crate::icons::Icon,
    state: nsh_core::prelude::Signal<Projection>,
) -> nsh_core::prelude::AnyElement {
    use nsh_core::prelude::*;
    svg(nsh_core::element::ImageSrc::svg_markup(
        &crate::icons::nsh_source(kind).unwrap_or_default(),
    ))
    .size(px(vega_theme::Layout::ICON_SIZE))
    .flex_shrink_0()
    .text_color(move || crate::nsh::color(state.get().colors.text_primary))
    .tint_color(move || crate::nsh::color(state.get().colors.text_primary))
    .into_element()
}

fn native_code_action(
    binding: String,
    state: nsh_core::prelude::Signal<Projection>,
    commands: crate::nsh::OrderedCommands<Request>,
) -> nsh_core::prelude::AnyElement {
    use nsh_core::prelude::*;
    let target = element_ref();
    let owner = nsh_core::element::Owner::current();
    let scrolling = use_context::<ReadingScroll>();
    let measurement = Rc::new(RefCell::new(None));
    let layout_task = Rc::new(RefCell::new(None));
    let layout_commands = commands.clone();
    let layout_binding = binding.clone();
    let measured_commands = commands.clone();
    let measured_binding = binding.clone();
    effect(move || {
        let session = state.with(|projection| projection.session.clone());
        if let Some(scrolling) = scrolling {
            scrolling.0.get();
        }
        let commands = measured_commands.clone();
        let binding = measured_binding.clone();
        *measurement.borrow_mut() = Some(spawn(async move {
            if let Some(measured) = target.measure().await {
                commands.send(Request::CodeBounds(
                    session,
                    binding,
                    [
                        measured.page.x.0,
                        measured.page.y.0,
                        measured.frame.size.width.0,
                        measured.frame.size.height.0,
                    ],
                ));
            }
        }));
    });
    view()
        .size(px(vega_theme::Layout::COPY_ACTION_SIZE))
        .items_center()
        .justify_center()
        .rounded(px(vega_theme::Layout::SETTINGS_CONTROL_RADIUS))
        .hover(move |style| style.bg(crate::nsh::color(state.get().colors.bg_hover)))
        .element_ref(&target)
        .label(move || state.with(|_| vega_i18n::t("ui.copy_code")))
        .tooltip(text(move || state.with(|_| vega_i18n::t("ui.copy_code"))))
        .child(code_icon(crate::icons::Icon::Copy, state))
        .select_none()
        .role(AccessibilityRole::Button)
        .on_layout(move |_: &Bounds<Points>| {
            let session = state.with(|projection| projection.session.clone());
            let commands = layout_commands.clone();
            let binding = layout_binding.clone();
            *layout_task.borrow_mut() = Some(owner.run(|| {
                spawn(async move {
                    if let Some(measured) = target.measure().await {
                        commands.send(Request::CodeBounds(
                            session,
                            binding,
                            [
                                measured.page.x.0,
                                measured.page.y.0,
                                measured.frame.size.width.0,
                                measured.frame.size.height.0,
                            ],
                        ));
                    }
                })
            }));
        })
        .on_press(move || {
            owner.run(|| {
                commands.send(Request::Code(
                    state.with(|projection| projection.session.clone()),
                    binding.clone(),
                ))
            })
        })
        .into_element()
}

fn native_code(
    code: &CodeBlock,
    state: nsh_core::prelude::Signal<Projection>,
) -> Vec<nsh_core::prelude::AnyElement> {
    use nsh_core::prelude::*;
    let tokens = code
        .language
        .as_deref()
        .and_then(|language| vega_markdown::highlight(&code.code, language))
        .unwrap_or_default();
    let mut spans = Vec::new();
    let mut cursor = 0;
    for token in tokens {
        if token.start_byte < cursor || token.end_byte > code.code.len() {
            continue;
        }
        if let Some(plain) = code.code.get(cursor..token.start_byte) {
            spans.push(text(plain.to_owned()).into_element());
        }
        if let Some(value) = code.code.get(token.start_byte..token.end_byte) {
            let kind = token.kind;
            let span = text(value.to_owned()).text_color(move || {
                state.with(|projection| {
                    crate::nsh::color(
                        super::super::model::code_token_style(kind, &projection.colors).color,
                    )
                })
            });
            spans.push(if kind == vega_markdown::HighlightKind::Comment {
                span.italic().into_element()
            } else {
                span.into_element()
            });
        }
        cursor = token.end_byte;
    }
    if let Some(plain) = code.code.get(cursor..) {
        spans.push(text(plain.to_owned()).into_element());
    }
    spans
}

fn inline_binding(content: &[Inline]) -> Option<String> {
    content.iter().find_map(|inline| match inline {
        Inline::Custom(node) => node.attributes.get("binding").cloned(),
        Inline::Strong { content }
        | Inline::Emphasis { content }
        | Inline::Strikethrough { content }
        | Inline::Link { content, .. } => inline_binding(content),
        _ => None,
    })
}

fn native_inlines(
    spans: &[Inline],
    state: nsh_core::prelude::Signal<Projection>,
    commands: crate::nsh::OrderedCommands<Request>,
) -> Vec<nsh_core::prelude::AnyElement> {
    use nsh_core::prelude::*;
    spans
        .iter()
        .map(|span| match span {
            Inline::Custom(node) => {
                let leaf = text(node.text.clone()).test_id(
                    node.attributes
                        .get("binding")
                        .map(String::as_str)
                        .unwrap_or_default(),
                );
                if node.name == "vega-reading-inline-code" {
                    leaf.font_family(super::super::model::MONOFONT)
                        .rounded(px(vega_theme::Layout::MARKDOWN_INLINE_CODE_RADIUS))
                        .into_element()
                } else {
                    leaf.into_element()
                }
            }
            Inline::Strong { content } => text("")
                .children(native_inlines(content, state, commands.clone()))
                .font_weight(FontWeight::BOLD)
                .into_element(),
            Inline::Emphasis { content } => text("")
                .children(native_inlines(content, state, commands.clone()))
                .italic()
                .into_element(),
            Inline::Strikethrough { content } => text("")
                .children(native_inlines(content, state, commands.clone()))
                .line_through()
                .into_element(),
            Inline::Link { url, content, .. } => {
                let action = commands.clone();
                let url = url.clone();
                let binding = inline_binding(content).unwrap_or_default();
                let owner = nsh_core::element::Owner::current();
                text("")
                    .children(native_inlines(content, state, commands.clone()))
                    .underline()
                    .text_color(move || {
                        state.with(|projection| crate::nsh::color(projection.colors.accent))
                    })
                    .role(AccessibilityRole::Link)
                    .on_press(move || {
                        owner.run(|| {
                            action.send(Request::Link(
                                state.with(|projection| projection.session.clone()),
                                url.clone(),
                                binding.clone(),
                            ))
                        })
                    })
                    .into_element()
            }
            _ => text(span.plain_text()).into_element(),
        })
        .collect()
}

fn list_boundary(block: &Block, first: bool) -> &Block {
    if let Block::Custom(node) = block
        && matches!(
            node.name.as_str(),
            "vega-reading-list" | "vega-reading-list-item"
        )
        && let Some(child) = if first {
            node.children.first()
        } else {
            node.children.last()
        }
    {
        return list_boundary(child, first);
    }
    block
}

fn native_row_blocks(
    blocks: &[Block],
    state: nsh_core::prelude::Signal<Projection>,
    commands: crate::nsh::OrderedCommands<Request>,
) -> nsh_core::prelude::AnyElement {
    use nsh_core::prelude::*;
    if blocks.len() < 2
        || !blocks
            .iter()
            .any(|block| matches!(block, Block::Heading { .. }))
    {
        return native_blocks(blocks, state, commands, false);
    }
    view()
        .flex_col()
        .children(blocks.iter().enumerate().map(|(index, block)| {
            let previous = index.checked_sub(1).map(|previous| &blocks[previous]);
            view()
                .pt(px(reading_block_gap(previous, block)))
                .child(native_blocks(
                    std::slice::from_ref(block),
                    state,
                    commands.clone(),
                    false,
                ))
        }))
        .into_element()
}

fn native_blocks(
    blocks: &[Block],
    state: nsh_core::prelude::Signal<Projection>,
    commands: crate::nsh::OrderedCommands<Request>,
    in_quote: bool,
) -> nsh_core::prelude::AnyElement {
    use nsh_core::prelude::*;
    let children = blocks
        .iter()
        .map(|block| match block {
            Block::Paragraph { content } => text("")
                .children(native_inlines(content, state, commands.clone()))
                .select_text()
                .line_height(nsh_core::LineHeight::Multiple(
                    vega_theme::Typography::CONVERSATION_LINE_HEIGHT,
                ))
                .into_element(),
            Block::Heading { level, content, .. } => {
                let (size, weight) = super::super::heading_style(*level);
                text("")
                    .children(native_inlines(content, state, commands.clone()))
                    .select_text()
                    .text_size(px(size))
                    .font_weight(FontWeight(weight.0 as u16))
                    .line_height(nsh_core::LineHeight::Multiple(
                        vega_theme::Typography::CONVERSATION_LINE_HEIGHT,
                    ))
                    .into_element()
            }
            Block::Blockquote { children, .. } => view()
                .flex_col()
                .pl(px(vega_theme::Layout::MARKDOWN_CODE_PADDING))
                .border_l_1()
                .border_color(move || {
                    state.with(|projection| crate::nsh::color(projection.colors.border_subtle))
                })
                .child(native_blocks(children, state, commands.clone(), true))
                .into_element(),
            Block::Custom(node) if node.name == "vega-reading-list" => {
                native_blocks(&node.children, state, commands.clone(), in_quote)
            }
            Block::Custom(node) if node.name == "vega-reading-list-item" => {
                let bullet = node.text == "•";
                let line_height = nsh_core::LineHeight::Multiple(
                    vega_theme::Typography::CONVERSATION_LINE_HEIGHT,
                );
                let thinking = state.with_untracked(|projection| projection.thinking);
                let marker = text("")
                    .child(
                        text(node.text.clone()).test_id(
                            node.attributes
                                .get("binding")
                                .map(String::as_str)
                                .unwrap_or_default(),
                        ),
                    )
                    .select_text()
                    .line_height(line_height)
                    .font_weight(if bullet && !thinking {
                        FontWeight::BOLD
                    } else {
                        FontWeight::NORMAL
                    })
                    .text_color(move || {
                        state.with(|projection| {
                            crate::nsh::color(if bullet || projection.thinking {
                                projection.colors.text_primary
                            } else {
                                projection.colors.text_secondary
                            })
                        })
                    });
                let checkbox = node
                    .attributes
                    .get("checkbox")
                    .zip(node.attributes.get("checkbox-binding"))
                    .map(|(label, id)| {
                        let checked = label == "[x]";
                        text("")
                            .child(text(label.clone()).test_id(id))
                            .select_text()
                            .line_height(line_height)
                            .mr(px(4.))
                            .text_color(move || {
                                state.with(|projection| {
                                    crate::nsh::color(if projection.thinking {
                                        projection.colors.text_primary
                                    } else if checked {
                                        projection.colors.success
                                    } else {
                                        projection.colors.text_tertiary
                                    })
                                })
                            })
                    });
                let row = view()
                    .flex_row()
                    .child(
                        view()
                            .w(px(vega_theme::Layout::MARKDOWN_LIST_INDENT))
                            .flex_shrink(0.)
                            .child(marker),
                    )
                    .children(checkbox);
                let (first, rest) = node.children.split_at(node.children.len().min(1));
                let row = row.child(view().min_w(px(0.)).flex_1().child(native_blocks(
                    first,
                    state,
                    commands.clone(),
                    in_quote,
                )));
                if rest.is_empty() {
                    row.into_element()
                } else {
                    view()
                        .flex_col()
                        .child(row)
                        .children(rest.iter().enumerate().map(|(index, block)| {
                            let previous = &node.children[index];
                            let first = list_boundary(block, true);
                            let last = list_boundary(previous, false);
                            let gap = if matches!(first, Block::Blockquote { .. }) || matches!(last, Block::Blockquote { .. }) {
                                vega_theme::Layout::MARKDOWN_BLOCK_GAP
                            } else if matches!(block, Block::Custom(node) if node.name == "vega-reading-list")
                                && matches!(first, Block::Paragraph { .. }) {
                                vega_theme::Layout::MARKDOWN_LIST_ITEM_GAP
                            } else if in_quote {
                                vega_theme::Layout::MARKDOWN_BLOCK_GAP
                            } else {
                                vega_theme::Layout::MARKDOWN_LIST_PARAGRAPH_GAP
                            };
                            view()
                                .pl(px(vega_theme::Layout::MARKDOWN_LIST_INDENT))
                                .pt(px(gap))
                                .child(native_blocks(
                                    std::slice::from_ref(block),
                                    state,
                                    commands.clone(),
                                    in_quote,
                                ))
                        }))
                        .into_element()
                }
            }
            Block::CodeBlock(code) => {
                let binding = code.meta.clone().unwrap_or_default();
                let content = text("")
                    .child(
                        text("")
                            .children(native_code(code, state))
                            .test_id(&binding),
                    )
                    .select_text()
                    .font_family(super::super::model::MONOFONT)
                    .text_size(px(vega_theme::Typography::CODE))
                    .line_height(nsh_core::LineHeight::Multiple(
                        vega_theme::Typography::CODE_LINE_HEIGHT,
                    ))
                    .app_class(Some("whitespace-nowrap".into()))
                    .px(px(vega_theme::Layout::MARKDOWN_CODE_PADDING))
                    .pt(px(vega_theme::Layout::COPY_ACTION_GAP))
                    .pb(px(vega_theme::Layout::MARKDOWN_CODE_PADDING));
                let language =
                    vega_markdown::language_display_name(code.language.as_deref()).to_owned();
                view()
                    .flex_col()
                    .rounded(px(vega_theme::Layout::TOOL_DETAIL_RADIUS))
                    .bg(move || {
                        state.with(|projection| crate::nsh::color(projection.colors.code_bg))
                    })
                    .child(
                        view()
                            .w_full()
                            .flex_row()
                            .items_center()
                            .px(px(vega_theme::Layout::MARKDOWN_CODE_PADDING))
                            .pt(px(vega_theme::Layout::MARKDOWN_CODE_PADDING))
                            .gap(px(vega_theme::Layout::COPY_ACTION_GAP * 2.))
                            .child(code_icon(crate::icons::Icon::Code, state))
                            .child(
                                text(language)
                                    .select_none()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .font_weight(nsh_core::prelude::FontWeight(500))
                                    .text_color(move || {
                                        crate::nsh::color(state.get().colors.text_primary)
                                    }),
                            )
                            .child(native_code_action(binding.clone(), state, commands.clone())),
                    )
                    .child(scroll().horizontal().scrollbar(true).child(content))
                    .into_element()
            }
            Block::Custom(node) if node.name == "vega-reading-table" => {
                let weights = node.attributes.clone();
                provide_context(TableWeights(weights));
                native_blocks(&node.children, state, commands.clone(), in_quote)
            }
            Block::Table(table) => {
                let weights = use_context::<TableWeights>()
                    .map(|weights| weights.0)
                    .unwrap_or_default();
                let rows = std::iter::once(&table.head)
                    .chain(&table.rows)
                    .enumerate()
                    .map(|(row, cells)| {
                        view()
                            .w_full()
                            .flex_row()
                            .border_b_1()
                            .border_color(move || {
                                state.with(|projection| {
                                    crate::nsh::color(projection.colors.border_subtle)
                                })
                            })
                            .children(
                                cells
                                    .iter()
                                    .enumerate()
                                    .map(|(column, cell)| {
                                        let cell = text("")
                                            .children(native_inlines(
                                                &cell.content,
                                                state,
                                                commands.clone(),
                                            ))
                                            .select_text()
                                            .px(px(vega_theme::Layout::MARKDOWN_TABLE_PADDING_X))
                                            .py(px(vega_theme::Layout::MARKDOWN_TABLE_PADDING_Y));
                                        let cell = if row == 0 {
                                            cell.font_weight(FontWeight::SEMIBOLD)
                                        } else {
                                            cell
                                        };
                                        let cell = match table.align.get(column) {
                                            Some(nsh_base::text_view::ColumnAlign::Center) => {
                                                cell.text_center()
                                            }
                                            Some(nsh_base::text_view::ColumnAlign::Right) => {
                                                cell.text_right()
                                            }
                                            _ => cell,
                                        };
                                        cell.w(px(
                                            vega_theme::Layout::MARKDOWN_TABLE_COLUMN_MIN_WIDTH,
                                        ))
                                        .flex_grow(
                                            weights
                                                .get(&format!("column-{column}"))
                                                .and_then(|weight| weight.parse::<f32>().ok())
                                                .unwrap_or(1.),
                                        )
                                        .flex_shrink(0.)
                                        .into_element()
                                    })
                                    .collect::<Vec<_>>(),
                            )
                            .into_element()
                    })
                    .collect::<Vec<_>>();
                scroll()
                    .horizontal()
                    .scrollbar(true)
                    .w_full()
                    .child(view().w_full().flex_col().children(rows))
                    .into_element()
            }
            Block::Rule => view()
                .h(px(1.))
                .w_full()
                .bg(move || {
                    state.with(|projection| crate::nsh::color(projection.colors.border_subtle))
                })
                .into_element(),
            _ => view().into_element(),
        })
        .collect::<Vec<_>>();
    let list_items = blocks
        .iter()
        .all(|block| matches!(block, Block::Custom(node) if node.name == "vega-reading-list-item"));
    let gap = if list_items {
        vega_theme::Layout::MARKDOWN_LIST_ITEM_GAP
    } else {
        vega_theme::Layout::MARKDOWN_BLOCK_GAP
    };
    view()
        .flex_col()
        .gap(px(gap))
        .children(children)
        .into_element()
}
