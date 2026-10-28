use super::*;
use std::collections::VecDeque;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HistoryHydration {
    pub(crate) older_cursor: Option<i64>,
    pub(crate) newer_cursor: Option<i64>,
    pub(crate) loading: bool,
    pub(crate) paused: bool,
    pub(crate) paused_direction: Option<HistoryPageDirection>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HistoryPageDirection {
    #[default]
    Older,
    Newer,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StreamEntryIdentity {
    pub(crate) key: String,
    pub(crate) message_id: Option<String>,
    pub(crate) pending_id: Option<String>,
    pub(crate) echo_kind: Option<String>,
    pub(crate) sequence: Option<i64>,
}

pub(crate) const STREAM_SAMPLE_CAPACITY: usize = 2048;

#[derive(Default)]
pub(crate) struct BoundedSamples {
    values: Mutex<VecDeque<u128>>,
}

impl BoundedSamples {
    pub(crate) fn push(&self, value: u128) {
        if let Ok(mut values) = self.values.lock() {
            if values.len() == STREAM_SAMPLE_CAPACITY {
                values.pop_front();
            }
            values.push_back(value);
        }
    }

    pub(crate) fn snapshot(&self) -> Vec<u128> {
        self.values
            .lock()
            .map(|values| values.iter().copied().collect())
            .unwrap_or_default()
    }
}

/// Monospace family for code rows (ui-spec §3 代码等宽档位；本机 macOS 以
/// Menlo 承担，spike 探针同款).
pub(crate) const MONOFONT: &str = "Menlo";

pub(crate) fn hydration_request(hydration: HistoryHydration, at_top: bool) -> Option<i64> {
    if !at_top || hydration.loading || hydration.paused {
        return None;
    }
    hydration.older_cursor
}

pub(crate) fn model_picker_starts_group(options: &[ModelOption], index: usize) -> bool {
    index == 0
        || options
            .get(index - 1)
            .map(|previous| previous.provider.as_str())
            != options.get(index).map(|option| option.provider.as_str())
}

pub(crate) fn model_picker_row_index(options: &[ModelOption], option_index: usize) -> usize {
    if options.is_empty() {
        return 0;
    }
    let last = option_index.min(options.len() - 1);
    (0..=last)
        .filter(|index| model_picker_starts_group(options, *index))
        .count()
        + last
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BlockOrigin {
    Committed,
    Pending,
}

/// Inline span style (the markdown inline subset this card maps).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SpanStyle {
    Plain,
    Strong,
    Emphasis,
    Strikethrough,
    /// Inline code: monospace on `code_bg`.
    Code,
    Link,
    Token(HighlightKind),
}

/// One resolved code-token style: token color + weight + italic.
pub(crate) struct TokenStyle {
    pub(crate) color: Rgba,
    pub(crate) weight: FontWeight,
    pub(crate) italic: bool,
}

pub(crate) fn code_token_style(kind: HighlightKind, colors: &ThemeColors) -> TokenStyle {
    match kind {
        HighlightKind::Keyword | HighlightKind::Type => TokenStyle {
            color: colors.text_primary,
            weight: FontWeight::BOLD,
            italic: false,
        },
        HighlightKind::String => TokenStyle {
            color: colors.success,
            weight: FontWeight::NORMAL,
            italic: false,
        },
        HighlightKind::Comment => TokenStyle {
            color: colors.text_tertiary,
            weight: FontWeight::NORMAL,
            italic: true,
        },
        HighlightKind::Number => TokenStyle {
            color: colors.warning,
            weight: FontWeight::NORMAL,
            italic: false,
        },
        HighlightKind::Function
        | HighlightKind::Operator
        | HighlightKind::Punctuation
        | HighlightKind::Variable
        | HighlightKind::Property
        | HighlightKind::Constant
        | HighlightKind::Escape
        | HighlightKind::Attribute => TokenStyle {
            color: colors.text_primary,
            weight: FontWeight::NORMAL,
            italic: false,
        },
    }
}

/// One styled text run inside a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StreamSpan {
    pub text: String,
    pub style: SpanStyle,
    pub link: Option<String>,
    pub link_occurrence: Option<usize>,
}

/// What a row represents (drives per-frame styling: font/bg/prefix).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LineKind {
    Heading(u8),
    Paragraph,
    /// List row; marker text (`"1."`, `"•"`) lives in [`StreamLine::marker`].
    ListItem,
    /// One structured table, including its header and body rows.
    Table,
    /// Code line (monospace on `code_bg`).
    Code,
    /// Thematic break (`---`).
    Rule,
    /// User message label row (「你」标记，卡片上方).
    UserLabel,
    /// User message content line inside the bg_elevated card; the flags mark
    /// the first/last line for top/bottom rounding and border edges.
    UserLine {
        first: bool,
        last: bool,
    },
    /// Blank spacer row between message blocks.
    Spacer,
}

/// One uniform-height display line. Produced once per `(block_id, version)`
/// at materialization time; per-frame rendering only clones it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StreamLine {
    /// Owning mdstream block id (diagnostics/tests).
    pub block_id: u64,
    pub kind: LineKind,
    /// Tasklist checkbox (`- [x]` / `- [ ]`); `None` for regular rows.
    pub checked: Option<bool>,
    /// Literal marker for list rows; empty otherwise.
    pub marker: String,
    /// Nesting depth for list rows (2-space indent per level).
    pub depth: usize,
    pub(crate) in_list: bool,
    pub(crate) quotes: Vec<QuoteOwner>,
    pub spans: Vec<StreamSpan>,
    pub table: Option<Arc<StreamTable>>,
    pub(crate) code_block: Option<Arc<StreamCodeBlock>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct QuoteOwner {
    pub(crate) identity: (u64, usize),
    pub(crate) list_depth: Option<usize>,
}

impl QuoteOwner {
    pub(crate) fn list_inset(self) -> f32 {
        self.list_depth.map_or(0., |depth| {
            (depth + 1) as f32 * Layout::MARKDOWN_LIST_INDENT
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StreamCodeBlock {
    pub(crate) identity: (u64, usize),
    pub(crate) language: Option<String>,
    pub(crate) code: Arc<str>,
}

/// Cached styled cells; the first row is the header. Column geometry belongs
/// to the renderer, never to whitespace padding in the text content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StreamTable {
    pub ordinal: usize,
    pub alignments: Vec<TableAlignment>,
    pub rows: Vec<Vec<Vec<StreamSpan>>>,
    column_width_units: Vec<usize>,
}

impl StreamTable {
    pub(super) fn column_weights(&self) -> Vec<f32> {
        self.column_width_units
            .iter()
            .map(|units| {
                let demand = (*units as f32 * Typography::CONVERSATION * 0.56
                    + 2.0 * Layout::MARKDOWN_TABLE_PADDING_X)
                    .clamp(
                        Layout::MARKDOWN_TABLE_COLUMN_MIN_WIDTH,
                        Layout::MARKDOWN_TABLE_COLUMN_DEMAND_CAP,
                    );
                (demand - Layout::MARKDOWN_TABLE_COLUMN_MIN_WIDTH).max(Typography::CONVERSATION)
            })
            .collect()
    }

    pub(super) fn column_widths(&self, available: f32) -> Vec<f32> {
        let weights = self.column_weights();
        let total: f32 = weights.iter().sum();
        let extra =
            (available - weights.len() as f32 * Layout::MARKDOWN_TABLE_COLUMN_MIN_WIDTH).max(0.0);
        weights
            .into_iter()
            .map(|weight| Layout::MARKDOWN_TABLE_COLUMN_MIN_WIDTH + extra * weight / total)
            .collect()
    }
}

impl StreamLine {
    pub(crate) fn new(block_id: u64, kind: LineKind) -> Self {
        Self {
            block_id,
            kind,
            checked: None,
            marker: String::new(),
            depth: 0,
            in_list: false,
            quotes: Vec::new(),
            spans: Vec::new(),
            table: None,
            code_block: None,
        }
    }

    pub(crate) fn starts_list_item(&self) -> bool {
        !self.marker.is_empty()
    }

    pub(crate) fn list_marker_quote_depth(&self) -> Option<usize> {
        if self.kind != LineKind::ListItem || !self.starts_list_item() {
            return None;
        }
        self.quotes
            .iter()
            .position(|owner| owner.list_depth == Some(self.depth))
    }

    pub(crate) fn list_marker_inset(&self) -> f32 {
        let parent_inset = self
            .list_marker_quote_depth()
            .and_then(|depth| depth.checked_sub(1))
            .and_then(|depth| self.quotes.get(depth))
            .map_or(0., |owner| owner.list_inset());
        (self.depth as f32 * Layout::MARKDOWN_LIST_INDENT - parent_inset).max(0.)
    }

    pub(crate) fn quote_list_inset(&self) -> f32 {
        self.quotes.last().map_or(0., |owner| owner.list_inset())
    }

    pub(crate) fn list_content_inset(&self) -> f32 {
        if self.in_list {
            (self.depth + 1) as f32 * Layout::MARKDOWN_LIST_INDENT
        } else {
            0.
        }
    }

    pub(crate) fn continues_code_block(&self, previous: &Self) -> bool {
        self.kind == LineKind::Code
            && previous.kind == LineKind::Code
            && self
                .code_block
                .as_ref()
                .zip(previous.code_block.as_ref())
                .is_some_and(|(current, previous)| Arc::ptr_eq(current, previous))
    }
}

/// Display width of a string: CJK/fullwidth characters count as 2 columns
/// for bounded tool/permission summary projections (spike §5.2 CJK caution).
pub(crate) fn display_width(text: &str) -> usize {
    text.chars()
        .map(|ch| {
            let scalar = ch as u32;
            let wide = (0x1100..=0x115F).contains(&scalar) // Hangul Jamo
                || (0x2E80..=0xA4CF).contains(&scalar) // CJK 部首~Yi
                || (0xAC00..=0xD7A3).contains(&scalar) // Hangul 音节
                || (0xF900..=0xFAFF).contains(&scalar) // CJK 兼容表意
                || (0xFE30..=0xFE4F).contains(&scalar) // CJK 兼容形式
                || (0xFF00..=0xFF60).contains(&scalar) // 全角形式
                || (0x1F300..=0x1FAFF).contains(&scalar); // emoji
            usize::from(wide) + 1
        })
        .sum()
}

/// Flattens inline spans onto [`SpanStyle`] (§5.3: 行内样式自研映射).
pub(crate) fn flatten_inlines(spans: &[Inline], out: &mut Vec<StreamSpan>) {
    for span in spans {
        match span {
            Inline::Text(text) => {
                let offset: usize = out.iter().map(|span| span.text.len()).sum();
                let mut spans = autolinks::text_spans(text);
                for span in &mut spans {
                    if let Some(occurrence) = &mut span.link_occurrence {
                        *occurrence += offset;
                    }
                }
                out.extend(spans);
            }
            Inline::Code(code) => out.push(StreamSpan {
                text: code.clone(),
                style: SpanStyle::Code,
                link: None,
                link_occurrence: None,
            }),
            Inline::Emphasis(inner) => restyle(out, inner, SpanStyle::Emphasis),
            Inline::Strong(inner) => restyle(out, inner, SpanStyle::Strong),
            Inline::Strikethrough(inner) => restyle(out, inner, SpanStyle::Strikethrough),
            Inline::Link {
                url, spans: inner, ..
            } => restyle_link(out, inner, url),
        }
    }
}

pub(crate) fn restyle(out: &mut Vec<StreamSpan>, inner: &[Inline], style: SpanStyle) {
    let start = out.len();
    flatten_inlines(inner, out);
    for span in &mut out[start..] {
        if span.style == SpanStyle::Plain || span.style == SpanStyle::Link {
            span.style = style;
        }
    }
}

fn restyle_link(out: &mut Vec<StreamSpan>, inner: &[Inline], url: &str) {
    let start = out.len();
    let occurrence = out.iter().map(|span| span.text.len()).sum();
    restyle(out, inner, SpanStyle::Link);
    for span in &mut out[start..] {
        span.link = Some(url.to_owned());
        span.link_occurrence = Some(occurrence);
    }
}

/// Coalesces adjacent same-style spans and drops empties (materialization-time
/// hygiene so per-frame row building stays flat and cheap).
pub(crate) fn coalesce(spans: Vec<StreamSpan>) -> Vec<StreamSpan> {
    let mut merged: Vec<StreamSpan> = Vec::with_capacity(spans.len());
    for span in spans {
        if span.text.is_empty() {
            continue;
        }
        match merged.last_mut() {
            Some(last)
                if last.style == span.style
                    && last.link == span.link
                    && last.link_occurrence == span.link_occurrence =>
            {
                last.text.push_str(&span.text)
            }
            _ => merged.push(span),
        }
    }
    merged
}

/// Flattens one materialized block (committed or pending) into
/// [`StreamLine`]s — the RenderNode → row mapping (§5.3, 纯函数可测).
/// `origin` decides whether committed code fences may be highlighted.
pub(crate) fn flatten_nodes(
    block_id: u64,
    nodes: &[RenderNode],
    origin: BlockOrigin,
) -> Vec<StreamLine> {
    let mut lines = Vec::new();
    let mut identity = 0;
    for node in nodes {
        flatten_node(block_id, node, None, origin, &mut lines, &mut identity);
    }
    lines
}

pub(crate) fn flatten_node(
    block_id: u64,
    node: &RenderNode,
    list_depth: Option<usize>,
    origin: BlockOrigin,
    out: &mut Vec<StreamLine>,
    identity: &mut usize,
) {
    let node_identity = *identity;
    *identity += 1;
    let depth = list_depth.unwrap_or_default();
    match node {
        RenderNode::Paragraph { spans } => {
            let mut inline = Vec::new();
            flatten_inlines(spans, &mut inline);
            if inline.is_empty() {
                return;
            }
            let mut line = StreamLine::new(block_id, LineKind::Paragraph);
            line.depth = depth;
            line.spans = coalesce(inline);
            out.push(line);
        }
        RenderNode::Heading { level, spans } => {
            let mut inline = Vec::new();
            flatten_inlines(spans, &mut inline);
            if inline.is_empty() {
                return;
            }
            let mut line = StreamLine::new(block_id, LineKind::Heading(*level));
            line.depth = depth;
            line.spans = coalesce(inline);
            out.push(line);
        }
        RenderNode::CodeBlock { language, code } => {
            let code_block = Arc::new(StreamCodeBlock {
                identity: (block_id, node_identity),
                language: language.clone(),
                code: code.as_str().into(),
            });
            let highlighted = match origin {
                BlockOrigin::Committed => language
                    .as_deref()
                    .and_then(|language| vega_markdown::highlight(code, language)),
                BlockOrigin::Pending => None,
            };
            // 逐物理行一行，保留代码缩进；仅吞掉尾换行产生的末尾空行。
            let raw: Vec<&str> = code.split('\n').collect();
            let mut offset = 0usize;
            let total = raw.len();
            for (index, code_line) in raw.iter().enumerate() {
                if index + 1 == total && code_line.is_empty() {
                    break;
                }
                let line_start = offset;
                let line_end = offset + code_line.len();
                let mut line = StreamLine::new(block_id, LineKind::Code);
                line.code_block = Some(code_block.clone());
                line.depth = depth;
                line.spans = coalesce(code_line_spans(
                    code,
                    line_start,
                    line_end,
                    highlighted.as_deref(),
                ));
                out.push(line);
                offset = line_end + 1; // skip the '\n'
            }
        }
        RenderNode::List(list) => flatten_list(
            block_id,
            list,
            list_depth.map_or(0, |depth| depth + 1),
            origin,
            out,
            identity,
        ),
        RenderNode::BlockQuote { children } => {
            let owner = QuoteOwner {
                identity: (block_id, node_identity),
                list_depth,
            };
            let start = out.len();
            for child in children {
                flatten_node(block_id, child, list_depth, origin, out, identity);
            }
            for line in &mut out[start..] {
                line.quotes.insert(0, owner);
            }
        }
        RenderNode::Table(table) => flatten_table(block_id, table, out),
        RenderNode::ThematicBreak => out.push(StreamLine::new(block_id, LineKind::Rule)),
    }
}

/// Slices the block-level highlight spans onto one code line `[start, end)`:
/// covered runs become [`SpanStyle::Token`], gaps stay plain (高亮映射的行
/// 切片；spans 有序且不重叠，切片保持顺序).
pub(crate) fn code_line_spans(
    code: &str,
    start: usize,
    end: usize,
    highlighted: Option<&[HighlightSpan]>,
) -> Vec<StreamSpan> {
    let Some(highlighted) = highlighted else {
        return vec![StreamSpan {
            text: code[start..end].to_string(),
            style: SpanStyle::Plain,
            link: None,
            link_occurrence: None,
        }];
    };
    let mut spans = Vec::new();
    let mut cursor = start;
    for span in highlighted {
        let span_start = span.start_byte.max(start);
        let span_end = span.end_byte.min(end);
        if span_start >= span_end {
            continue;
        }
        if span_start > cursor {
            spans.push(StreamSpan {
                text: code[cursor..span_start].to_string(),
                style: SpanStyle::Plain,
                link: None,
                link_occurrence: None,
            });
        }
        spans.push(StreamSpan {
            text: code[span_start..span_end].to_string(),
            style: SpanStyle::Token(span.kind),
            link: None,
            link_occurrence: None,
        });
        cursor = span_end;
    }
    if cursor < end {
        spans.push(StreamSpan {
            text: code[cursor..end].to_string(),
            style: SpanStyle::Plain,
            link: None,
            link_occurrence: None,
        });
    }
    spans
}

pub(crate) fn flatten_list(
    block_id: u64,
    list: &ListBlock,
    depth: usize,
    origin: BlockOrigin,
    out: &mut Vec<StreamLine>,
    identity: &mut usize,
) {
    for (index, item) in list.items.iter().enumerate() {
        let marker = if list.ordered {
            format!("{}.", list.start + index as u64)
        } else {
            "•".to_string()
        };
        let start = out.len();
        for child in &item.children {
            match child {
                // 嵌套列表携带 depth+1（§5.3 嵌套列表分支）。
                RenderNode::List(nested) => {
                    flatten_list(block_id, nested, depth + 1, origin, out, identity)
                }
                other => flatten_node(block_id, other, Some(depth), origin, out, identity),
            }
        }
        for line in &mut out[start..] {
            line.in_list = true;
        }
        if let Some(first) = out.get_mut(start) {
            first.marker = marker;
            first.depth = depth;
            first.checked = item.checked;
            if first.kind == LineKind::Paragraph {
                first.kind = LineKind::ListItem;
            }
        }
    }
}

pub(crate) fn flatten_table(block_id: u64, table: &TableBlock, out: &mut Vec<StreamLine>) {
    if table.header.is_empty() {
        return;
    }
    let columns = table.header.len();
    let rows: Vec<Vec<Vec<StreamSpan>>> = std::iter::once(&table.header)
        .chain(table.rows.iter())
        .map(|row| {
            (0..columns)
                .map(|column| {
                    let mut spans = Vec::new();
                    if let Some(cell) = row.get(column) {
                        flatten_inlines(&cell.spans, &mut spans);
                    }
                    coalesce(spans)
                })
                .collect()
        })
        .collect();
    let column_width_units = (0..columns)
        .map(|column| {
            rows.iter()
                .map(|row| {
                    let text: String = row[column].iter().map(|span| span.text.as_str()).collect();
                    text.split('\n')
                        .map(display_width)
                        .max()
                        .unwrap_or_default()
                })
                .max()
                .unwrap_or_default()
        })
        .collect();
    let mut line = StreamLine::new(block_id, LineKind::Table);
    line.table = Some(Arc::new(StreamTable {
        ordinal: out.iter().filter(|line| line.table.is_some()).count(),
        alignments: (0..columns)
            .map(|column| {
                table
                    .alignments
                    .get(column)
                    .copied()
                    .unwrap_or(TableAlignment::None)
            })
            .collect(),
        rows,
        column_width_units,
    }));
    out.push(line);
}

// ─── diff/materialization engine (P3: 冻结块只物化一次) ──────────────────────

/// A frozen committed block's materialized lines.
pub(crate) struct CachedBlock {
    pub(crate) version: u64,
    pub(crate) lines: Vec<StreamLine>,
}

/// Counters shared with the bench harness (spike 计数器方法).
#[derive(Default)]
pub(crate) struct StreamCounters {
    /// Render callbacks executed (fps numerator).
    pub frames: AtomicU64,
    /// Per-frame element-tree build times, ns (render 回调耗时，spike 口径).
    pub render_ns: BoundedSamples,
    /// Per-frame visible-row build times, ns (变高 list 的 render_item 回调).
    pub row_build_ns: BoundedSamples,
    pub row_callbacks: AtomicU64,
    /// Committed blocks materialized for the first time.
    pub committed_materializations: AtomicU64,
    /// Already-cached committed blocks re-materialized (P3 指标：普通流式期间
    /// 应为 0；Update.invalidated 的合法版本升级也计入，普通流不含).
    pub frozen_rematerializations: AtomicU64,
    /// Pending tail re-flattens (once per delta-carrying update).
    pub pending_materializations: AtomicU64,
}

impl StreamCounters {
    /// Records one render-callback duration (spike 口径的 frame build 时间).
    pub(crate) fn record_render(&self, started: Instant) {
        self.frames.fetch_add(1, Ordering::Relaxed);
        let elapsed = started.elapsed().as_nanos();
        self.render_ns.push(elapsed);
    }

    pub(crate) fn record_row_callback(&self, elapsed: u128) {
        self.row_callbacks.fetch_add(1, Ordering::Relaxed);
        self.row_build_ns.push(elapsed);
    }
}

/// Incremental row model: reconciles [`StreamSnapshot`]s into a flat row list
/// while materializing each committed block exactly once per version.
#[derive(Default)]
pub(crate) struct StreamModel {
    content_revision: u64,
    pub(crate) committed_ids: Vec<u64>,
    /// Parallel to `committed_ids`: the materialized version of each block
    /// (invalidation detection without per-frame HashMap lookups).
    pub(crate) committed_versions: Vec<u64>,
    pub(crate) committed_lines: Vec<StreamLine>,
    pub(crate) pending_lines: Vec<StreamLine>,
    /// `(block_id, version)` of the pending rows currently materialized.
    pub(crate) pending_key: Option<(u64, u64)>,
    pub(crate) cache: std::collections::HashMap<u64, CachedBlock>,
}

impl StreamModel {
    pub(crate) fn content_revision(&self) -> u64 {
        self.content_revision
    }

    /// Total row count (committed + pending).
    pub(crate) fn row_count(&self) -> usize {
        self.committed_lines.len() + self.pending_lines.len()
    }

    /// Reconciles one snapshot: appends new committed blocks, re-materializes
    /// invalidated ones (version bump), and replaces the pending tail.
    ///
    /// Returns whether any row content changed (the anchor's `content_grew`).
    pub(crate) fn sync(
        &mut self,
        snapshot: &StreamSnapshot<'_>,
        counters: &StreamCounters,
    ) -> bool {
        // reset 语义（tech-spec §5.0）：id 序列不再前缀兼容 → 全量重建。
        let prefix_compatible = snapshot.blocks.len() >= self.committed_ids.len()
            && self
                .committed_ids
                .iter()
                .zip(snapshot.blocks.iter())
                .all(|(old, new)| *old == new.block_id);
        if !prefix_compatible {
            self.committed_ids.clear();
            self.committed_versions.clear();
            self.committed_lines.clear();
            self.pending_lines.clear();
            self.pending_key = None;
            self.cache.clear();
        }
        let mut changed = !prefix_compatible;
        let append_from = if prefix_compatible {
            self.committed_ids.len()
        } else {
            0
        };

        // 已缓存块被 invalidated（version 升级）时整段重拼 committed 行；
        // 正常流式路径（纯追加）只走 extend。版本核对走并行 Vec（O(n) 数组
        // 比较，热路径上避免全量 HashMap 查找）。
        let mut invalidated = self.committed_versions.len() != append_from;
        if !invalidated {
            for (index, block) in snapshot.blocks[..append_from].iter().enumerate() {
                if self.committed_versions[index] != block.version {
                    invalidated = true;
                    break;
                }
            }
        }
        if invalidated {
            self.committed_lines.clear();
            self.committed_ids.clear();
            self.committed_versions.clear();
            for block in &snapshot.blocks {
                let lines = self.materialize_committed(block, counters);
                self.committed_ids.push(block.block_id);
                self.committed_versions.push(block.version);
                self.committed_lines.extend(lines);
            }
            changed = true;
        } else {
            for block in &snapshot.blocks[append_from..] {
                let lines = self.materialize_committed(block, counters);
                self.committed_ids.push(block.block_id);
                self.committed_versions.push(block.version);
                self.committed_lines.extend(lines);
                changed = true;
            }
        }

        // pending 尾块：版本变化时才轻量重排（不入冻结缓存）；重复同步
        // 同一快照不改行内容，也不标 changed。
        let pending_key = snapshot
            .pending
            .map(|pending| (pending.block_id, pending.version));
        if pending_key != self.pending_key {
            self.pending_lines = snapshot
                .pending
                .map(|pending| flatten_nodes(pending.block_id, pending.nodes, BlockOrigin::Pending))
                .unwrap_or_default();
            if snapshot.pending.is_some() {
                counters
                    .pending_materializations
                    .fetch_add(1, Ordering::Relaxed);
            }
            self.pending_key = pending_key;
            changed = true;
        }
        if changed {
            self.content_revision = self.content_revision.wrapping_add(1);
        }
        changed
    }

    /// Materializes one committed block honoring the freeze contract: a cache
    /// hit with the same version is free; a first sighting bumps
    /// `committed_materializations`; a version bump on an already-cached block
    /// bumps `frozen_rematerializations` (P3 指标).
    pub(crate) fn materialize_committed(
        &mut self,
        block: &BlockView<'_>,
        counters: &StreamCounters,
    ) -> Vec<StreamLine> {
        match self.cache.get(&block.block_id) {
            Some(cached) if cached.version == block.version => cached.lines.clone(),
            Some(_) => {
                counters
                    .frozen_rematerializations
                    .fetch_add(1, Ordering::Relaxed);
                let lines = flatten_nodes(block.block_id, block.nodes, BlockOrigin::Committed);
                self.cache.insert(
                    block.block_id,
                    CachedBlock {
                        version: block.version,
                        lines: lines.clone(),
                    },
                );
                lines
            }
            None => {
                counters
                    .committed_materializations
                    .fetch_add(1, Ordering::Relaxed);
                let lines = flatten_nodes(block.block_id, block.nodes, BlockOrigin::Committed);
                self.cache.insert(
                    block.block_id,
                    CachedBlock {
                        version: block.version,
                        lines: lines.clone(),
                    },
                );
                lines
            }
        }
    }
}

#[derive(Clone)]
pub(crate) struct MessageCopy {
    pub(super) compaction_menu:
        std::rc::Rc<std::cell::RefCell<Option<std::rc::Rc<super::selection_menu::MenuBuilder>>>>,
    pub(super) id: u64,
    text: std::rc::Rc<std::cell::RefCell<String>>,
    live_projection_unavailable: bool,
    generated_notices:
        std::rc::Rc<std::cell::RefCell<Vec<super::generated_notice::GeneratedNoticeFragment>>>,
    display_model_text: std::rc::Rc<std::cell::RefCell<Option<String>>>,
    stream_finished: std::rc::Rc<std::cell::Cell<bool>>,
    selection_scope: gpui_kit::base::TextSelectionScopeId,
    selection_handle: std::rc::Rc<std::cell::RefCell<Option<gpui_kit::base::TextSelectionHandle>>>,
    selection_generation: std::rc::Rc<std::cell::Cell<u64>>,
    selected_text: std::rc::Rc<std::cell::RefCell<Option<(u64, String)>>>,
    local_selection_range: std::rc::Rc<std::cell::RefCell<Option<std::ops::Range<usize>>>>,
    visible_text: std::rc::Rc<std::cell::RefCell<Option<Arc<str>>>>,
    content_generation: std::rc::Rc<std::cell::Cell<u64>>,
    projected_identity: std::rc::Rc<std::cell::Cell<Option<(u64, u64)>>>,
    projected_selection: std::rc::Rc<std::cell::RefCell<Option<std::ops::Range<usize>>>>,
    selection_subscriptions: std::rc::Rc<std::cell::RefCell<Vec<gpui_kit::Subscription>>>,
}

pub(super) struct PreparedMessage {
    text: String,
    notices: Vec<super::generated_notice::GeneratedNoticeFragment>,
    display_text: Option<String>,
    stream: MarkdownStream,
    model: StreamModel,
}

impl PreparedMessage {
    pub(super) fn new(
        text: &str,
        notices: Vec<super::generated_notice::GeneratedNoticeFragment>,
        counters: &StreamCounters,
    ) -> Self {
        let mut stream = MarkdownStream::new();
        stream.append(text);
        stream.finish();
        let mut model = StreamModel::default();
        let display_text = if notices.is_empty() {
            model.sync(&stream.snapshot(), counters);
            None
        } else {
            let display_text = super::generated_notice::project_text(text, &notices);
            let mut projected = MarkdownStream::new();
            projected.append(&display_text);
            projected.finish();
            model.sync(&projected.snapshot(), counters);
            model.content_revision = 1;
            Some(display_text)
        };
        Self {
            text: text.to_owned(),
            notices,
            display_text,
            stream,
            model,
        }
    }

    pub(super) fn into_message(
        self,
        counters: &StreamCounters,
    ) -> (MessageCopy, Box<MarkdownStream>, StreamModel) {
        let copy = MessageCopy::new("");
        let has_notices = self.display_text.is_some();
        *copy.text.borrow_mut() = self.text;
        *copy.generated_notices.borrow_mut() = self.notices;
        *copy.display_model_text.borrow_mut() = self.display_text;
        copy.stream_finished.set(true);
        let mut model = self.model;
        if has_notices {
            copy.sync_model(&self.stream, &mut model, counters);
        }
        (copy, Box::new(self.stream), model)
    }
}

impl MessageCopy {
    pub(crate) fn new(text: &str) -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            compaction_menu: Default::default(),
            text: std::rc::Rc::new(std::cell::RefCell::new(text.to_owned())),
            live_projection_unavailable: false,
            generated_notices: Default::default(),
            display_model_text: Default::default(),
            stream_finished: Default::default(),
            selection_scope: gpui_kit::base::TextSelectionScopeId::new(),
            selection_handle: std::rc::Rc::new(std::cell::RefCell::new(None)),
            selection_generation: std::rc::Rc::new(std::cell::Cell::new(0)),
            selected_text: std::rc::Rc::new(std::cell::RefCell::new(None)),
            local_selection_range: std::rc::Rc::new(std::cell::RefCell::new(None)),
            visible_text: std::rc::Rc::new(std::cell::RefCell::new(None)),
            content_generation: std::rc::Rc::new(std::cell::Cell::new(0)),
            projected_identity: std::rc::Rc::new(std::cell::Cell::new(None)),
            projected_selection: Default::default(),
            selection_subscriptions: std::rc::Rc::new(std::cell::RefCell::new(Vec::new())),
        }
    }

    pub(super) fn from_live_text(text: &str) -> Self {
        let projected = vega_conversation::privacy::safe_live_text(text);
        let unavailable = vega_i18n::t("domain.privacy.check_unavailable");
        let live_projection_unavailable = projected != text
            && projected == unavailable
            && vega_conversation::privacy::safe_live_text("") == unavailable;
        let mut copy = Self::new(&projected);
        copy.live_projection_unavailable = live_projection_unavailable;
        copy
    }

    pub(super) fn live_projection_unavailable(&self) -> bool {
        self.live_projection_unavailable
    }

    pub(super) fn refresh_live_projection(&self) -> bool {
        if !self.live_projection_unavailable {
            return false;
        }
        let text = vega_i18n::t("domain.privacy.check_unavailable");
        if self.original_text() == text {
            return false;
        }
        self.replace_text(text);
        true
    }

    pub(super) fn content_generation(&self) -> u64 {
        self.content_generation.get()
    }

    pub(super) fn set_projected_identity(&self, identity: u64) -> bool {
        let next = (identity, self.content_generation());
        self.projected_identity
            .replace(Some(next))
            .is_some_and(|previous| previous != next)
    }

    pub(crate) fn original_text(&self) -> String {
        self.text.borrow().clone()
    }

    pub(crate) fn display_text(&self) -> String {
        super::generated_notice::project_text(&self.text.borrow(), &self.generated_notices.borrow())
    }

    pub(crate) fn generated_notices(
        &self,
    ) -> Vec<super::generated_notice::GeneratedNoticeFragment> {
        self.generated_notices.borrow().clone()
    }

    pub(crate) fn set_generated_notices(
        &self,
        notices: Vec<super::generated_notice::GeneratedNoticeFragment>,
    ) {
        if *self.generated_notices.borrow() == notices {
            return;
        }
        *self.generated_notices.borrow_mut() = notices;
        *self.display_model_text.borrow_mut() = None;
        self.invalidate_selection();
    }

    pub(crate) fn sync_model(
        &self,
        stream: &MarkdownStream,
        model: &mut StreamModel,
        counters: &StreamCounters,
    ) -> bool {
        if self.generated_notices.borrow().is_empty() {
            return model.sync(&stream.snapshot(), counters);
        }
        let text = self.display_text();
        if self.display_model_text.borrow().as_ref() == Some(&text) {
            return false;
        }
        let mut projected = MarkdownStream::new();
        projected.append(&text);
        if self.stream_finished.get() {
            projected.finish();
        }
        let revision = model.content_revision;
        *model = StreamModel {
            content_revision: revision,
            ..StreamModel::default()
        };
        model.sync(&projected.snapshot(), counters);
        model.content_revision = revision.wrapping_add(1);
        *self.display_model_text.borrow_mut() = Some(text);
        self.invalidate_selection();
        true
    }

    pub(crate) fn finish_model(
        &self,
        stream: &mut MarkdownStream,
        model: &mut StreamModel,
        counters: &StreamCounters,
    ) {
        stream.finish();
        self.stream_finished.set(true);
        *self.display_model_text.borrow_mut() = None;
        self.sync_model(stream, model, counters);
    }

    pub(crate) fn append(&self, delta: &str) {
        self.text.borrow_mut().push_str(delta);
        self.content_generation
            .set(self.content_generation.get().wrapping_add(1));
        self.invalidate_selection();
    }

    pub(crate) fn replace_text(&self, text: &str) {
        *self.text.borrow_mut() = text.to_owned();
        self.content_generation
            .set(self.content_generation.get().wrapping_add(1));
        self.invalidate_selection();
    }

    pub(super) fn append_retained(&self, delta: &str) {
        self.text.borrow_mut().push_str(delta);
    }

    pub(super) fn replace_retained(&self, text: &str) {
        *self.text.borrow_mut() = text.to_owned();
    }

    pub(super) fn clear_retained(&self) {
        self.replace_text("");
        *self.visible_text.borrow_mut() = None;
        *self.projected_selection.borrow_mut() = None;
        self.selection_subscriptions.borrow_mut().clear();
        self.selection_handle.borrow_mut().take();
        self.projected_identity.set(None);
    }

    pub(super) fn set_projected_selection(&self, range: Option<std::ops::Range<usize>>) {
        *self.projected_selection.borrow_mut() = range;
    }

    pub(super) fn publish_retained(
        &self,
        identity: u64,
        text: Arc<str>,
        map: impl FnOnce(std::ops::Range<usize>) -> Option<std::ops::Range<usize>>,
    ) {
        let selection = self.projected_selection.borrow().clone().and_then(map);
        self.content_generation
            .set(self.content_generation.get().wrapping_add(1));
        if let Some(range) = selection.filter(|range| text.get(range.clone()).is_some()) {
            self.set_local_selection_range(range.clone());
            self.set_selected_text(self.selection_generation(), text[range.clone()].to_owned());
            self.set_projected_selection(Some(range));
            self.projected_identity
                .set(Some((identity, self.content_generation())));
        } else {
            self.invalidate_selection();
        }
        self.set_visible_text(text);
    }

    pub(crate) fn selection_scope(&self) -> gpui_kit::base::TextSelectionScopeId {
        self.selection_scope
    }

    pub(crate) fn selection_generation(&self) -> u64 {
        self.selection_generation.get()
    }

    pub(crate) fn ensure_selection_handle(
        &self,
        focus: &gpui_kit::FocusHandle,
        window: &Window,
        cx: &mut App,
    ) -> gpui_kit::base::TextSelectionHandle {
        if let Some(handle) = self.selection_handle.borrow().as_ref() {
            return handle.clone();
        }
        let handle = gpui_kit::base::TextSelectionHandle::new("", cx);
        let selected = self.selected_text.clone();
        let generation = self.selection_generation.clone();
        handle.copy_with(
            move |_| {
                selected
                    .borrow()
                    .as_ref()
                    .filter(|(current, _)| *current == generation.get())
                    .map(|(_, text)| text.clone())
                    .unwrap_or_default()
            },
            cx,
        );
        let focus = focus.clone();
        handle.focus_with(move |window, cx| focus.focus(window, cx), cx);
        let selected = self.selected_text.clone();
        let generation = self.selection_generation.clone();
        let local_range = self.local_selection_range.clone();
        let projected_range = self.projected_selection.clone();
        let subscription = handle.subscribe(
            move |event, _| {
                if matches!(
                    event,
                    gpui_kit::base::TextSelectionEvent::SelectionChanged(_)
                        | gpui_kit::base::TextSelectionEvent::Cleared
                ) {
                    *selected.borrow_mut() = None;
                    *local_range.borrow_mut() = None;
                    *projected_range.borrow_mut() = None;
                    generation.set(generation.get().wrapping_add(1));
                }
            },
            cx,
        );
        self.selection_subscriptions.borrow_mut().push(subscription);
        self.selection_subscriptions
            .borrow_mut()
            .push(handle.refresh_window_on_change(window, cx));
        *self.selection_handle.borrow_mut() = Some(handle.clone());
        handle
    }

    pub(crate) fn visible_text(&self) -> Option<String> {
        self.visible_text
            .borrow()
            .as_ref()
            .map(|text| text.to_string())
    }

    pub(crate) fn set_visible_text(&self, text: Arc<str>) {
        *self.visible_text.borrow_mut() = Some(text);
    }

    pub(crate) fn set_selected_text(&self, generation: u64, text: String) {
        *self.selected_text.borrow_mut() = (!text.is_empty()).then_some((generation, text));
    }

    pub(crate) fn invalidate_selection(&self) {
        self.selection_generation
            .set(self.selection_generation.get().wrapping_add(1));
        *self.selected_text.borrow_mut() = None;
        *self.local_selection_range.borrow_mut() = None;
        *self.projected_selection.borrow_mut() = None;
    }

    pub(super) fn selection_has_cursor(&self, cx: &App) -> bool {
        self.selection_handle
            .borrow()
            .as_ref()
            .and_then(|handle| {
                handle
                    .snapshot(cx)
                    .map(|snapshot| snapshot.cursor().entity_id() == Some(handle.entity_id()))
            })
            .unwrap_or_else(|| self.has_selected_text() || self.local_selection_range().is_some())
    }

    pub(super) fn clear_hidden_selection(&self, window: &mut Window, cx: &mut App) {
        let handle = self.selection_handle.borrow().clone();
        if let Some(handle) = handle {
            if handle.snapshot(cx).is_some() || handle.has_local_selection(cx) {
                gpui_kit::base::TextSelection::clear(window, cx);
            }
            handle.set_local_selection(false, cx);
        }
        self.invalidate_selection();
    }

    pub(super) fn local_selection_range(&self) -> Option<std::ops::Range<usize>> {
        self.local_selection_range.borrow().clone()
    }

    pub(super) fn set_local_selection_range(&self, range: std::ops::Range<usize>) {
        *self.local_selection_range.borrow_mut() = Some(range);
    }

    pub(crate) fn has_selected_text(&self) -> bool {
        self.selected_text
            .borrow()
            .as_ref()
            .is_some_and(|(generation, text)| {
                *generation == self.selection_generation.get() && !text.is_empty()
            })
    }

    pub(crate) fn copy_selected_text(&self, cx: &mut App) {
        if let Some((generation, text)) = self.selected_text.borrow().as_ref()
            && *generation == self.selection_generation.get()
            && !text.is_empty()
        {
            cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(text.clone()));
        }
    }

    pub(super) fn has_text(&self) -> bool {
        !self.text.borrow().is_empty()
    }

    pub(super) fn copy(&self, cx: &mut App) {
        let text = self.display_text();
        if !text.is_empty() {
            cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(text));
        }
    }
}

impl Default for MessageCopy {
    fn default() -> Self {
        Self::new("")
    }
}

pub(crate) enum StreamEntry {
    Subagents {
        group: Entity<super::subagents::SubagentGroup>,
    },
    Screenshot {
        card: Entity<screenshots::ScreenshotCard>,
    },
    RunActivity {
        group: Entity<RunActivityGroup>,
    },
    RunActivitySegment {
        group: Entity<RunActivityGroup>,
        segment: usize,
    },
    /// Content-free compaction activity at its original live stream boundary.
    ContextCompaction {
        model: String,
        record: vega_conversation::types::ContextCompactionStatusRecord,
        restored: bool,
    },
    /// Local user echo (Composer send): static rows, materialized once.
    User {
        lines: Vec<StreamLine>,
        copy: MessageCopy,
    },
    UserImages {
        images: Vec<attachments::ImagePreview>,
    },
    /// One assistant turn: a whole [`MarkdownStream`] plus its diff model.
    Assistant {
        copy: MessageCopy,
        stream: Box<MarkdownStream>,
        model: StreamModel,
        /// Bounded terminal reason; never contains a provider response body.
        failure: Option<RunFailureKind>,
    },
    /// One audited tool call rendered directly as a compact activity row.
    Tool {
        card: Entity<ToolCard>,
    },
    /// Two or more adjacent audited calls rendered as one activity item.
    ToolGroup {
        group: Entity<ToolActivityGroup>,
    },
    ArtifactGroup {
        group: Entity<ArtifactGroup>,
    },
    /// Sole active permission request/response handoff card.
    Permission {
        card: Entity<PermissionCard>,
    },
    /// One durable Plan review card.
    Plan {
        card: Entity<PlanCard>,
    },
    Summary {
        card: Entity<SummaryCard>,
    },
    /// Content-free historical Skill provenance; never a live capability.
    SkillActivation {
        activation: vega_conversation::history::SkillHistoryActivation,
    },
}

impl StreamEntry {
    pub(crate) fn row_count(&self, cx: &App) -> usize {
        match self {
            StreamEntry::Subagents { group } => group.read(cx).row_count(cx),
            StreamEntry::RunActivity { .. } => 1,
            StreamEntry::RunActivitySegment { group, segment } => {
                group.read(cx).segment_row_count(*segment, cx)
            }
            StreamEntry::User { lines, .. } => lines.len(),
            StreamEntry::UserImages { .. } | StreamEntry::Screenshot { .. } => 1,
            StreamEntry::Assistant { model, failure, .. } => {
                model.row_count() + usize::from(failure.is_some())
            }
            StreamEntry::Tool { card } => card.read(cx).row_count(),
            StreamEntry::ToolGroup { group } => group.read(cx).row_count(cx),
            StreamEntry::ArtifactGroup { group } => group.read(cx).row_count(),
            StreamEntry::Permission { card } => card.read(cx).row_count(),
            StreamEntry::Plan { card } => card.read(cx).row_count(),
            StreamEntry::Summary { card } => usize::from(
                card.read(cx).summary().outcome
                    != vega_conversation::types::TaskSummaryOutcome::Completed,
            ),
            StreamEntry::SkillActivation { .. } => 1,
            StreamEntry::ContextCompaction { record, .. } => usize::from(!matches!(
                record.status,
                vega_conversation::types::ContextCompactionStatus::Succeeded
                    | vega_conversation::types::ContextCompactionStatus::Cancelled
            )),
        }
    }
}

/// Synthetic block id base for user echo rows (StreamLine diagnostics only;
/// real mdstream BlockIds start at 1, so the top of the range never collides).
pub(crate) const USER_BLOCK_BASE: u64 = u64::MAX - (1 << 32);
