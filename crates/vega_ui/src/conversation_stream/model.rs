use super::*;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HistoryHydration {
    pub(crate) older_cursor: Option<i64>,
    pub(crate) loading: bool,
    pub(crate) paused: bool,
}

/// Monospace family for code rows (ui-spec §3 代码等宽档位；本机 macOS 以
/// Menlo 承担，spike 探针同款).
pub(crate) const MONOFONT: &str = "Menlo";

// ─── anchor state machine (P4, pure & unit-tested) ───────────────────────────

/// Pure anchor state machine (P4): 贴底自动跟随；上翻 >1 屏后不再自动跳转；
/// 回到底部恢复。
pub(crate) mod anchor {
    use super::ANCHOR_EPSILON_PX;

    /// Whether the view currently follows the stream tail.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum AnchorState {
        /// Pinned to the bottom: new content triggers a jump to the bottom.
        Following,
        /// Detached by the user scrolling up more than one viewport: no more
        /// auto-jumps until the user returns to the bottom.
        Detached,
    }

    /// What the view should do this frame.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum AnchorAction {
        /// Leave the scroll offset alone.
        StayPut,
        /// Jump to the bottom (this frame's deferred scroll-to-bottom).
        StickToBottom,
    }

    /// Outcome of one anchor step: the (possibly updated) state and action.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) struct AnchorDecision {
        pub state: AnchorState,
        pub action: AnchorAction,
    }

    /// Initial state: an opened stream starts pinned to the bottom.
    pub(crate) const INITIAL: AnchorState = AnchorState::Following;

    /// Advances the anchor state machine by one frame.
    ///
    /// - `distance_from_bottom_px`: current scroll distance to the document
    ///   bottom (0 = pinned), from the scroll handle geometry.
    /// - `viewport_height_px`: visible list height — the "one screen" P4
    ///   threshold. Non-positive values (layout not run yet) disable the
    ///   detach rule.
    /// - `content_grew`: whether new content arrived since the last frame.
    pub(crate) fn step(
        state: AnchorState,
        distance_from_bottom_px: f32,
        viewport_height_px: f32,
        content_grew: bool,
    ) -> AnchorDecision {
        let at_bottom = distance_from_bottom_px <= ANCHOR_EPSILON_PX;
        match state {
            AnchorState::Detached => {
                if at_bottom {
                    // 回到底部恢复（P4）。
                    AnchorDecision {
                        state: AnchorState::Following,
                        action: AnchorAction::StickToBottom,
                    }
                } else {
                    AnchorDecision {
                        state: AnchorState::Detached,
                        action: AnchorAction::StayPut,
                    }
                }
            }
            AnchorState::Following => {
                if viewport_height_px > 0.0 && distance_from_bottom_px > viewport_height_px {
                    // 上翻超过 1 屏：停止自动跳转（P4）。
                    AnchorDecision {
                        state: AnchorState::Detached,
                        action: AnchorAction::StayPut,
                    }
                } else if content_grew {
                    AnchorDecision {
                        state: AnchorState::Following,
                        action: AnchorAction::StickToBottom,
                    }
                } else {
                    AnchorDecision {
                        state: AnchorState::Following,
                        action: AnchorAction::StayPut,
                    }
                }
            }
        }
    }
}

/// Scroll offset that keeps the viewport anchored at the page boundary after
/// prepending `prepended_rows` uniform-height rows above the visible content
/// (S8-T45/C7 页边界保 anchor). The offset grows more negative while
/// scrolling down, so the exact prepend height is subtracted.
pub(crate) fn anchored_prepend_offset(current: Pixels, prepended_rows: usize) -> Pixels {
    current - px(prepended_rows as f32 * ROW_HEIGHT)
}

/// Pure scroll-up hydration request gate (S8-T45/C7): a page may be requested
/// only when the viewport is at the top edge, older history exists, no page
/// is in flight, and no failure pause is armed.
pub(crate) fn hydration_request(hydration: HistoryHydration, at_top: bool) -> Option<i64> {
    if !at_top || hydration.loading || hydration.paused {
        return None;
    }
    hydration.older_cursor
}

// ─── render instructions: RenderNode → StreamLine mapping (§5.3) ─────────────

/// Whether a block's code fences may be highlighted (S3-T18 高亮整合策略):
/// committed blocks go through the T16 tree-sitter query; the pending tail
/// (unclosed fence) degrades to plain monospace (tech-spec §5.1).
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
    /// Link label: underlined, secondary color.
    Link,
    /// Code token from the T16 highlighter (monospace row; the token →
    /// theme mapping is [`code_token_style`]).
    Token(HighlightKind),
}

/// One resolved code-token style: token color + weight + italic.
pub(crate) struct TokenStyle {
    pub(crate) color: Rgba,
    pub(crate) weight: FontWeight,
    pub(crate) italic: bool,
}

/// S3-T18 高亮整合的**唯一映射表**：HighlightKind → 既有 ui-spec §2 色值
/// token（无新色值）：Keyword/Type → text_primary 加粗，String → success，
/// Comment → text_tertiary 斜体，Number → warning，其余 → text_primary。
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
}

/// What a row represents (drives per-frame styling: font/bg/prefix).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LineKind {
    Heading(u8),
    Paragraph,
    /// List row; marker text (`"1."`, `"•"`) lives in [`StreamLine::marker`].
    ListItem,
    /// Table header row.
    TableHeader,
    /// Table body row.
    TableRow,
    /// Code line (monospace on `code_bg`).
    Code,
    /// Block-quote line (left bar + secondary color).
    Quote,
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
    pub spans: Vec<StreamSpan>,
}

impl StreamLine {
    pub(crate) fn new(block_id: u64, kind: LineKind) -> Self {
        Self {
            block_id,
            kind,
            checked: None,
            marker: String::new(),
            depth: 0,
            spans: Vec::new(),
        }
    }
}

/// Display width of a string: CJK/fullwidth characters count as 2 columns so
/// table column padding stays visually aligned in mixed text (spike §5.2 CJK
/// caution; pure function, unit-tested).
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
            Inline::Text(text) => out.push(StreamSpan {
                text: text.clone(),
                style: SpanStyle::Plain,
            }),
            Inline::Code(code) => out.push(StreamSpan {
                text: code.clone(),
                style: SpanStyle::Code,
            }),
            Inline::Emphasis(inner) => restyle(out, inner, SpanStyle::Emphasis),
            Inline::Strong(inner) => restyle(out, inner, SpanStyle::Strong),
            Inline::Strikethrough(inner) => restyle(out, inner, SpanStyle::Strikethrough),
            Inline::Link { spans: inner, .. } => restyle(out, inner, SpanStyle::Link),
        }
    }
}

/// Appends `inner` spans, restyling the plain ones that came from `inner`
/// (nested styles compose: only plain runs adopt the outer style).
pub(crate) fn restyle(out: &mut Vec<StreamSpan>, inner: &[Inline], style: SpanStyle) {
    let start = out.len();
    flatten_inlines(inner, out);
    for span in &mut out[start..] {
        if span.style == SpanStyle::Plain {
            span.style = style;
        }
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
            Some(last) if last.style == span.style => last.text.push_str(&span.text),
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
    for node in nodes {
        flatten_node(block_id, node, 0, origin, &mut lines);
    }
    lines
}

pub(crate) fn flatten_node(
    block_id: u64,
    node: &RenderNode,
    depth: usize,
    origin: BlockOrigin,
    out: &mut Vec<StreamLine>,
) {
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
            // T18 高亮整合：committed 块按语言走 T16 tree-sitter 高亮；
            // pending（未闭合 fence）/未支持语言降级纯文本等宽（§5.1）。
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
        RenderNode::List(list) => flatten_list(block_id, list, depth, origin, out),
        RenderNode::BlockQuote { children } => {
            let start = out.len();
            for child in children {
                flatten_node(block_id, child, depth, origin, out);
            }
            for line in &mut out[start..] {
                line.kind = LineKind::Quote;
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
            });
        }
        spans.push(StreamSpan {
            text: code[span_start..span_end].to_string(),
            style: SpanStyle::Token(span.kind),
        });
        cursor = span_end;
    }
    if cursor < end {
        spans.push(StreamSpan {
            text: code[cursor..end].to_string(),
            style: SpanStyle::Plain,
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
                RenderNode::List(nested) => flatten_list(block_id, nested, depth + 1, origin, out),
                other => flatten_node(block_id, other, depth, origin, out),
            }
        }
        if let Some(first) = out.get_mut(start) {
            first.marker = marker;
            first.depth = depth;
            first.checked = item.checked;
            first.kind = LineKind::ListItem;
        }
    }
}

pub(crate) fn flatten_table(block_id: u64, table: &TableBlock, out: &mut Vec<StreamLine>) {
    if table.header.is_empty() {
        return;
    }
    // 列宽 = 各列显示宽最大值（CJK 计 2），对齐按 GFM 分隔行（§5.3 表格分支）。
    let columns = table.header.len();
    let mut widths = vec![0usize; columns];
    let mut header_cells = Vec::with_capacity(columns);
    for (column, cell) in table.header.iter().enumerate() {
        let text = inline_plain(&cell.spans);
        widths[column] = widths[column].max(display_width(&text));
        header_cells.push(text);
    }
    let mut body: Vec<Vec<String>> = Vec::with_capacity(table.rows.len());
    for row in &table.rows {
        let mut cells = Vec::with_capacity(columns);
        for (column, cell) in row.iter().enumerate().take(columns) {
            let text = inline_plain(&cell.spans);
            widths[column] = widths[column].max(display_width(&text));
            cells.push(text);
        }
        // 畸形表的缺列补空（保持每行列数一致）。
        cells.resize(columns, String::new());
        body.push(cells);
    }
    let alignment_of = |column: usize| {
        table
            .alignments
            .get(column)
            .copied()
            .unwrap_or(TableAlignment::None)
    };
    let pad = |text: &str, width: usize, alignment: TableAlignment| {
        let padding = width.saturating_sub(display_width(text));
        match alignment {
            TableAlignment::Right => format!("{}{text}", " ".repeat(padding)),
            TableAlignment::Center => {
                let left = padding / 2;
                format!("{}{text}{}", " ".repeat(left), " ".repeat(padding - left))
            }
            _ => format!("{text}{}", " ".repeat(padding)),
        }
    };
    let mut header_line = StreamLine::new(block_id, LineKind::TableHeader);
    for (column, text) in header_cells.iter().enumerate() {
        if column > 0 {
            header_line.spans.push(StreamSpan {
                text: " │ ".to_string(),
                style: SpanStyle::Plain,
            });
        }
        header_line.spans.push(StreamSpan {
            text: pad(text, widths[column], alignment_of(column)),
            style: SpanStyle::Plain,
        });
    }
    out.push(header_line);
    for row in body {
        let mut line = StreamLine::new(block_id, LineKind::TableRow);
        for (column, text) in row.iter().enumerate() {
            if column > 0 {
                line.spans.push(StreamSpan {
                    text: " │ ".to_string(),
                    style: SpanStyle::Plain,
                });
            }
            line.spans.push(StreamSpan {
                text: pad(text, widths[column], alignment_of(column)),
                style: SpanStyle::Plain,
            });
        }
        out.push(line);
    }
}

/// Plain-text projection of inline spans (table cells lose inline styling in
/// this card's row model; content is preserved verbatim).
pub(crate) fn inline_plain(spans: &[Inline]) -> String {
    pub(crate) fn push(spans: &[Inline], out: &mut String) {
        for span in spans {
            match span {
                Inline::Text(text) | Inline::Code(text) => out.push_str(text),
                Inline::Emphasis(inner) | Inline::Strong(inner) | Inline::Strikethrough(inner) => {
                    push(inner, out)
                }
                Inline::Link { spans: inner, .. } => push(inner, out),
            }
        }
    }
    let mut out = String::new();
    push(spans, &mut out);
    out
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
    pub render_ns: Mutex<Vec<u128>>,
    /// Per-frame visible-row build times, ns (uniform_list range 回调).
    pub row_build_ns: Mutex<Vec<u128>>,
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
        if let Ok(mut samples) = self.render_ns.lock() {
            samples.push(elapsed);
        }
    }
}

/// Incremental row model: reconciles [`StreamSnapshot`]s into a flat row list
/// while materializing each committed block exactly once per version.
#[derive(Default)]
pub(crate) struct StreamModel {
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
    /// Total row count (committed + pending).
    pub(crate) fn row_count(&self) -> usize {
        self.committed_lines.len() + self.pending_lines.len()
    }

    /// Renders the rows in `range` (per-frame: clone cached lines only, P3).
    pub(crate) fn rows_in(&self, range: Range<usize>, colors: &ThemeColors) -> Vec<AnyElement> {
        range
            .filter_map(|index| self.row(index).map(|line| render_row(line, colors)))
            .collect()
    }

    /// Row accessor for `uniform_list`'s range callback.
    pub(crate) fn row(&self, index: usize) -> Option<&StreamLine> {
        let committed = self.committed_lines.len();
        if index < committed {
            self.committed_lines.get(index)
        } else {
            self.pending_lines.get(index - committed)
        }
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

// ─── message entries (T18 消息块结构) ────────────────────────────────────────

/// One conversation entry: a local user echo or one assistant markdown turn.
///
/// 架构师裁决（T18 裁决①）：user 消息块用「独立渲染路径挂 StreamSnapshot
/// 外侧」实现 —— 不进入 MarkdownStream，T15 管线零侵入；每段 assistant 流
/// 拥有独立的 final 终结语义（回放结束 `finish()`，tech-spec §5.4）。
pub(crate) enum StreamEntry {
    /// Local user echo (Composer send): static rows, materialized once.
    User { lines: Vec<StreamLine> },
    /// One assistant turn: a whole [`MarkdownStream`] plus its diff model.
    Assistant {
        stream: Box<MarkdownStream>,
        model: StreamModel,
    },
    /// One audited tool card. Expansion adds fixed-height virtual rows.
    Tool { card: Entity<ToolCard> },
    /// One route-owned artifact, placed immediately after its exact tool.
    Artifact { card: Entity<ArtifactCard> },
    /// Sole active permission request/response handoff card.
    Permission { card: Entity<PermissionCard> },
    /// One durable Plan review card.
    Plan { card: Entity<PlanCard> },
    /// One read-only per-task cost summary card (S7-T40), projected by
    /// `vega_conversation::summary` and applied by the app layer.
    Summary { card: Entity<SummaryCard> },
}

impl StreamEntry {
    pub(crate) fn row_count(&self, cx: &App) -> usize {
        match self {
            StreamEntry::User { lines } => lines.len(),
            StreamEntry::Assistant { model, .. } => model.row_count(),
            StreamEntry::Tool { card } => card.read(cx).row_count(),
            StreamEntry::Artifact { card } => card.read(cx).row_count(),
            StreamEntry::Permission { card } => card.read(cx).row_count(),
            StreamEntry::Plan { card } => card.read(cx).row_count(),
            StreamEntry::Summary { card } => card.read(cx).row_count(),
        }
    }
}

/// Synthetic block id base for user echo rows (StreamLine diagnostics only;
/// real mdstream BlockIds start at 1, so the top of the range never collides).
pub(crate) const USER_BLOCK_BASE: u64 = u64::MAX - (1 << 32);

/// Materializes a user echo block (T18 消息块结构): 「你」 label row, one card
/// line per source line (first/last flagged for rounding/border edges), and a
/// trailing spacer row separating it from the next message.
pub(crate) fn user_message_lines(block_id: u64, text: &str) -> Vec<StreamLine> {
    let mut lines = vec![StreamLine::new(block_id, LineKind::UserLabel)];
    let trimmed = text.trim_end_matches('\n');
    let raw: Vec<&str> = trimmed.split('\n').collect();
    let count = raw.len();
    for (index, part) in raw.iter().enumerate() {
        let mut line = StreamLine::new(
            block_id,
            LineKind::UserLine {
                first: index == 0,
                last: index + 1 == count,
            },
        );
        line.spans = coalesce(vec![StreamSpan {
            text: (*part).to_string(),
            style: SpanStyle::Plain,
        }]);
        lines.push(line);
    }
    lines.push(StreamLine::new(block_id, LineKind::Spacer));
    lines
}

/// Renders one visible range across all entries (the `uniform_list` range
/// callback; per-frame: clone-only, P3).
pub(crate) fn build_entry_rows(
    entries: &[StreamEntry],
    range: Range<usize>,
    counters: &StreamCounters,
    window: &mut Window,
    cx: &mut App,
) -> Vec<AnyElement> {
    let row_t0 = Instant::now();
    let colors = theme(cx).colors;
    let mut rows: Vec<AnyElement> = Vec::new();
    let mut offset = 0usize;
    for entry in entries {
        let count = entry.row_count(cx);
        let start = range.start.saturating_sub(offset);
        let end = range.end.saturating_sub(offset).min(count);
        if start < end {
            match entry {
                StreamEntry::User { lines } => {
                    rows.extend(
                        lines[start..end]
                            .iter()
                            .map(|line| render_row(line, &colors)),
                    );
                }
                StreamEntry::Assistant { model, .. } => {
                    rows.extend(model.rows_in(start..end, &colors));
                }
                StreamEntry::Tool { card } => {
                    rows.extend(
                        (start..end).map(|row| ToolCard::render_row(card.clone(), row, cx)),
                    );
                }
                StreamEntry::Artifact { card } => {
                    rows.extend(
                        (start..end)
                            .map(|row| ArtifactCard::render_row(card.clone(), row, window, cx)),
                    );
                }
                StreamEntry::Permission { card } => {
                    rows.extend(
                        (start..end)
                            .map(|row| PermissionCard::render_row(card.clone(), row, window, cx)),
                    );
                }
                StreamEntry::Plan { card } => {
                    rows.extend(
                        (start..end).map(|row| PlanCard::render_row(card.clone(), row, window, cx)),
                    );
                }
                StreamEntry::Summary { card } => {
                    rows.extend(
                        (start..end).map(|row| SummaryCard::render_row(card.clone(), row, cx)),
                    );
                }
            }
        }
        offset += count;
        if offset >= range.end {
            break;
        }
    }
    if let Ok(mut samples) = counters.row_build_ns.lock() {
        samples.push(row_t0.elapsed().as_nanos());
    }
    rows
}

// ─── per-frame row rendering ─────────────────────────────────────────────────

/// Builds the visible rows for one `uniform_list` range by cloning cached
/// [`StreamLine`]s (no materialization here — that is the P3 contract).
/// Single-model form used by the render_frame bench probe.
pub(crate) fn build_rows(
    model: &StreamModel,
    range: Range<usize>,
    counters: &StreamCounters,
    cx: &App,
) -> Vec<AnyElement> {
    let row_t0 = Instant::now();
    let colors = theme(cx).colors;
    let rows = model.rows_in(range, &colors);
    if let Ok(mut samples) = counters.row_build_ns.lock() {
        samples.push(row_t0.elapsed().as_nanos());
    }
    rows
}

/// Renders one [`StreamLine`] into a uniform-height row.
pub(crate) fn render_row(line: &StreamLine, colors: &ThemeColors) -> AnyElement {
    let mut row = div()
        .h(px(ROW_HEIGHT))
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .overflow_hidden()
        .flex_shrink_0()
        // 内容列留白（ui-spec §1 左右留白 ≥24px；T18 消息块成型）。
        .px(px(CONTENT_MIN_PADDING))
        // 会话消息正文 14px（ui-spec §3；标题/代码分支按档位覆盖）。
        .text_size(px(Typography::MESSAGE));
    match line.kind {
        LineKind::Spacer => return row.into_any_element(),
        LineKind::UserLabel => {
            return row
                .text_size(px(Typography::SIDEBAR))
                .text_color(colors.text_secondary)
                // 与卡片文字左缘对齐（卡片内缩 px_2）。
                .child(div().px_2().child("你"))
                .into_any_element();
        }
        LineKind::UserLine { first, last } => {
            // user 消息卡片（风格裁决：bg_elevated 圆角卡片 + 「你」标记、
            // 左对齐、1px border_subtle；非右对齐气泡）。
            row = row
                .bg(colors.bg_elevated)
                .border_color(colors.border_subtle);
            row = row.border_l_1().border_r_1();
            if first {
                row = row.border_t_1().rounded_tl_lg().rounded_tr_lg();
            }
            if last {
                row = row.border_b_1().rounded_bl_lg().rounded_br_lg();
            }
        }
        LineKind::Code => {
            row = row
                .bg(colors.code_bg)
                .font_family(MONOFONT.to_string())
                .text_size(px(Typography::CODE));
        }
        LineKind::Quote => {
            row = row.text_color(colors.text_secondary).child(
                // 引用左竖条（token 色，无字形风险）。
                div()
                    .w(px(2.))
                    .h(px(16.))
                    .mr_2()
                    .flex_shrink_0()
                    .bg(colors.border_subtle),
            );
        }
        LineKind::Rule => {
            return row
                .child(div().flex_1().h(px(1.)).bg(colors.border_subtle))
                .into_any_element();
        }
        LineKind::Heading(level) => {
            let (size, weight) = heading_style(level);
            row = row.text_size(px(size)).font_weight(weight);
        }
        LineKind::TableHeader => {
            row = row.bg(colors.bg_hover).font_weight(FontWeight::MEDIUM);
        }
        LineKind::ListItem => {
            let indent = "  ".repeat(line.depth);
            row = row.child(
                div()
                    .flex_shrink_0()
                    .text_color(colors.text_secondary)
                    .child(format!("{}{} ", indent, line.marker)),
            );
            if let Some(checked) = line.checked {
                row = row.child(
                    div()
                        .flex_shrink_0()
                        .mr_1()
                        .text_color(if checked {
                            colors.success
                        } else {
                            colors.text_tertiary
                        })
                        .child(if checked { "[x]" } else { "[ ]" }),
                );
            }
        }
        LineKind::Paragraph | LineKind::TableRow => {}
    }
    // 代码块/user 卡片的文字需要 bg 内再缩进：包进内层容器（gpui 的 px
    // 后写覆盖前写而非叠加，不能直接在行上再 px_2）。
    if matches!(line.kind, LineKind::Code | LineKind::UserLine { .. }) {
        let mut inner = div().w_full().flex().flex_row().items_center().px_2();
        for span in &line.spans {
            inner = inner.child(render_span(span, colors));
        }
        row = row.child(inner);
    } else {
        for span in &line.spans {
            row = row.child(render_span(span, colors));
        }
    }
    row.into_any_element()
}

/// Heading tier → (font size token, weight token) — 字号全部取自 ui-spec §3
/// 的 Typography 档位，不发明新字号.
pub(crate) fn heading_style(level: u8) -> (f32, FontWeight) {
    match level {
        1..=2 => (Typography::HEADING_PAGE, Typography::HEADING_PAGE_WEIGHT),
        3..=4 => (Typography::HEADING_BLOCK, Typography::HEADING_BLOCK_WEIGHT),
        _ => (Typography::MESSAGE, Typography::HEADING_CARD_WEIGHT),
    }
}

pub(crate) fn render_span(span: &StreamSpan, colors: &ThemeColors) -> AnyElement {
    let mut text = div().child(span.text.clone());
    text = match span.style {
        SpanStyle::Plain => text,
        SpanStyle::Strong => text.font_weight(FontWeight::BOLD),
        SpanStyle::Emphasis => text.italic(),
        SpanStyle::Strikethrough => text.line_through(),
        SpanStyle::Code => text
            .font_family(MONOFONT.to_string())
            .text_size(px(Typography::CODE))
            .bg(colors.code_bg)
            .px_1()
            .rounded_sm(),
        SpanStyle::Link => text.underline().text_color(colors.text_secondary),
        // 高亮 token：等宽字体/字号由 Code 行容器继承，只覆写映射表给出的
        // 颜色/字重/斜体。
        SpanStyle::Token(kind) => {
            let token = code_token_style(kind, colors);
            let mut styled = text.text_color(token.color).font_weight(token.weight);
            if token.italic {
                styled = styled.italic();
            }
            styled
        }
    };
    text.into_any_element()
}

// ─── sample document (演示注入载荷) ──────────────────────────────────────────

/// Builds the built-in demo document (`blocks` blocks: headings / inline
/// styles / tables / lists / code / quotes — 任务卡要求的样本集).
pub(crate) fn sample_document(blocks: usize) -> String {
    let cjk = [
        "这是一段中文文本，验证混排流式渲染。",
        "中文与 English 混排需要保持稳定。",
        "表格里的中文列宽按 2 列计。",
    ];
    let mut doc = String::with_capacity(blocks * 160);
    for index in 0..blocks {
        let zh = cjk[index % cjk.len()];
        match index % 8 {
            0 => doc.push_str(&format!("## 段落 {index}：流式 Markdown\n\n")),
            1 => doc.push_str(&format!(
                "段落 {index} 带 **加粗**、*斜体*、`行内代码`、\
                 [链接](https://example.com/{index}) 和 ~~删除线~~。{zh}\n\n"
            )),
            2 => doc.push_str(&format!(
                "| 列 A {index} | 列 B | 列 C |\n|:--|:-:|--:|\n| 1 | {zh} | 3 |\n| 4 | 5 | 6 |\n\n"
            )),
            3 => doc.push_str(&format!(
                "- 任务甲 {index}\n- [ ] 待办\n- [x] 已完成\n  - 嵌套项 `code`\n\n"
            )),
            4 => doc.push_str(&format!(
                "```rust\nfn demo_{index}() -> u64 {{\n    let v = {index} * 42;\n    v\n}}\n```\n\n"
            )),
            5 => doc.push_str(&format!("> 引用行一 {index}\n> 引用行二 {zh}\n\n")),
            6 => doc.push_str(&format!("1. 有序甲 {index}\n2. 有序乙\n\n")),
            _ => doc.push_str(&format!("普通收尾段落 {index}。{zh}\n\n")),
        }
    }
    doc
}

// (split_deltas moved to vega_markdown::replay — T18 公共回放器基建)

// ─── the conversation stream view ────────────────────────────────────────────
