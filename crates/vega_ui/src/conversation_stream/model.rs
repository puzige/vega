use super::*;
use std::collections::VecDeque;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HistoryHydration {
    pub(crate) older_cursor: Option<i64>,
    pub(crate) loading: bool,
    pub(crate) paused: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct StreamEntryIdentity {
    pub(crate) key: String,
    pub(crate) message_id: Option<String>,
    pub(crate) sequence: Option<i64>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StreamAnchorSnapshot {
    pub(crate) identity: Option<String>,
    pub(crate) offset_in_item: Pixels,
    pub(crate) following_tail: bool,
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

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.values
            .lock()
            .map(|values| values.len())
            .unwrap_or_default()
    }
}

/// Monospace family for code rows (ui-spec §3 代码等宽档位；本机 macOS 以
/// Menlo 承担，spike 探针同款).
pub(crate) const MONOFONT: &str = "Menlo";

// ─── top-level anchoring (P4, S8-T44) ────────────────────────────────────────
//
// The anchor semantics (贴底跟随 / 上翻 detach / 回底 resume) are carried by
// the pinned GPUI variable-height list itself: `ListState::set_follow_mode
// (FollowMode::Tail)` auto-scrolls to the end while following, any upward
// scroll event detaches, and layout re-engages once the viewport returns to
// within 1px of the bottom — the same epsilon the old pure state machine
// used (`ANCHOR_EPSILON_PX`). `ConversationStream` only reads
// `ListState::is_following_tail()` for the header indicator and re-engages
// explicitly (Tail mode) when a permission prompt must be visible.

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
    /// One structured table, including its header and body rows.
    Table,
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
    pub table: Option<Arc<StreamTable>>,
}

/// Cached styled cells; the first row is the header. Column geometry belongs
/// to the renderer, never to whitespace padding in the text content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StreamTable {
    pub ordinal: usize,
    pub alignments: Vec<TableAlignment>,
    pub rows: Vec<Vec<Vec<StreamSpan>>>,
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
            table: None,
        }
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
    let columns = table.header.len();
    let rows = std::iter::once(&table.header)
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

/// UI-only original text and stable action identity. Handlers share the buffer
/// so a click reads the newest delta without cloning the full body per frame.
#[derive(Clone)]
pub(crate) struct MessageCopy {
    pub(super) id: u64,
    text: std::rc::Rc<std::cell::RefCell<String>>,
}

impl MessageCopy {
    pub(crate) fn new(text: &str) -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            text: std::rc::Rc::new(std::cell::RefCell::new(text.to_owned())),
        }
    }

    pub(crate) fn append(&self, delta: &str) {
        self.text.borrow_mut().push_str(delta);
    }

    pub(super) fn has_text(&self) -> bool {
        !self.text.borrow().is_empty()
    }

    pub(super) fn copy(&self, cx: &mut App) {
        let text = self.text.borrow();
        if !text.is_empty() {
            cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(text.clone()));
        }
    }
}

impl Default for MessageCopy {
    fn default() -> Self {
        Self::new("")
    }
}

/// One conversation entry: a local user echo or one assistant markdown turn.
///
/// 架构师裁决（T18 裁决①）：user 消息块用「独立渲染路径挂 StreamSnapshot
/// 外侧」实现 —— 不进入 MarkdownStream，T15 管线零侵入；每段 assistant 流
/// 拥有独立的 final 终结语义（回放结束 `finish()`，tech-spec §5.4）。
pub(crate) enum StreamEntry {
    /// Content-free compaction activity at its original live stream boundary.
    ContextCompaction {
        model: String,
        record: vega_conversation::types::ContextCompactionStatusRecord,
        restored: bool,
    },
    /// Live reasoning is deliberately separate from persisted answer text.
    Thinking { card: Entity<ThinkingBlock> },
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
    Tool { card: Entity<ToolCard> },
    /// Two or more adjacent audited calls rendered as one activity item.
    ToolGroup { group: Entity<ToolActivityGroup> },
    /// One route-owned artifact, placed immediately after its exact tool.
    Artifact { card: Entity<ArtifactCard> },
    /// Sole active permission request/response handoff card.
    Permission { card: Entity<PermissionCard> },
    /// One durable Plan review card.
    Plan { card: Entity<PlanCard> },
    /// One read-only per-task cost summary card (S7-T40), projected by
    /// `vega_conversation::summary` and applied by the app layer.
    Summary { card: Entity<SummaryCard> },
    /// Content-free historical Skill provenance; never a live capability.
    SkillActivation {
        activation: vega_conversation::history::SkillHistoryActivation,
    },
}

impl StreamEntry {
    pub(crate) fn row_count(&self, cx: &App) -> usize {
        match self {
            StreamEntry::Thinking { card } => 1 + usize::from(card.read(cx).expanded),
            StreamEntry::User { lines, .. } => lines.len(),
            StreamEntry::UserImages { .. } => 1,
            StreamEntry::Assistant { model, failure, .. } => {
                model.row_count() + usize::from(failure.is_some())
            }
            StreamEntry::Tool { card } => card.read(cx).row_count(),
            StreamEntry::ToolGroup { group } => group.read(cx).row_count(cx),
            StreamEntry::Artifact { card } => card.read(cx).row_count(),
            StreamEntry::Permission { card } => card.read(cx).row_count(),
            StreamEntry::Plan { card } => card.read(cx).row_count(),
            StreamEntry::Summary { card } => usize::from(
                card.read(cx).summary().outcome
                    != vega_conversation::types::TaskSummaryOutcome::Completed,
            ),
            StreamEntry::SkillActivation { .. } | StreamEntry::ContextCompaction { .. } => 1,
        }
    }
}

/// Synthetic block id base for user echo rows (StreamLine diagnostics only;
/// real mdstream BlockIds start at 1, so the top of the range never collides).
pub(crate) const USER_BLOCK_BASE: u64 = u64::MAX - (1 << 32);
