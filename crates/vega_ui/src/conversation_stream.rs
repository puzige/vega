//! Virtualized conversation stream (S3-T17): renders a
//! [`vega_markdown::MarkdownStream`] as a `uniform_list` of uniform-height
//! rows with anchored tail-following and a temporary demo injector.
//!
//! Layering (tech-spec §5.1/§5.3 — the self-built parts of this card):
//!
//! ```text
//! MarkdownStream.append(delta) ─▶ snapshot()
//!     ├─ committed blocks (BlockId stable, frozen) ─▶ StreamModel diff by
//!     │                                              (block_id, version):
//!     │                                              only new/invalidated
//!     │                                              blocks materialize
//!     │                                              into StreamLines
//!     └─ pending tail block                        ─▶ light re-flatten
//!        uniform_list(range) ─▶ per-frame rows built by cloning StreamLines
//!                               (frozen rows never re-materialize — P3)
//! ```
//!
//! - **差量渲染**: [`StreamModel::sync`] materializes a committed block exactly
//!   once per `(block_id, version)`; frozen blocks keep their [`StreamLine`]s
//!   for the lifetime of the stream, so streaming appends only touch the tail
//!   (spike counter method: frozen re-materializations stay 0).
//! - **锚定跟随 (P4)**: pure state machine [`anchor::step`] — pinned at the
//!   bottom it follows new content; scrolling up more than one viewport
//!   detaches; returning to the bottom re-engages.
//! - Rows are single logical lines at a fixed height (the `uniform_list`
//!   contract); long lines truncate. Block types map per ui-spec §3 tokens.
//! - **演示注入**: the header button feeds the built-in ~200-block sample
//!   document into the stream at ~500 δ/s (S3 临时，T18 换 mock 回放器).
//!
//! The stream is memory-only (S3 has no message persistence): opening a
//! thread constructs an empty [`MarkdownStream`].

pub mod bench;

use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Context, FontWeight, MouseButton, MouseUpEvent, Render, Window, div, px,
    uniform_list,
};
use vega_conversation::types::Thread;
use vega_markdown::{
    BlockView, Inline, ListBlock, MarkdownStream, RenderNode, StreamSnapshot, TableAlignment,
    TableBlock,
};
use vega_theme::{ThemeColors, Typography, theme};

use crate::sidebar::CONTENT_MIN_PADDING;

/// Uniform row height (logical px). A `uniform_list` requires one fixed item
/// height for every row; 24px comfortably fits the 14px/1.6 message body line
/// (22.4px) and the 12.5px code line mandated by ui-spec §3.
pub(crate) const ROW_HEIGHT: f32 = 24.0;

/// Distance from the bottom (px) below which the view still counts as pinned
/// (absorbs sub-pixel wheel residue).
pub(crate) const ANCHOR_EPSILON_PX: f32 = 1.0;

/// Demo injection pacing: the 16ms tick polls; the injected count follows
/// `INJECT_RATE × elapsed` (≈500 δ/s 任务卡口径，自校正抵消主线程抖动).
const INJECT_TICK: Duration = Duration::from_millis(16);
const INJECT_RATE: usize = 500;

/// Monospace family for code rows (ui-spec §3 代码等宽档位；本机 macOS 以
/// Menlo 承担，spike 探针同款).
const MONOFONT: &str = "Menlo";

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

// ─── render instructions: RenderNode → StreamLine mapping (§5.3) ─────────────

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
    fn new(block_id: u64, kind: LineKind) -> Self {
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
fn flatten_inlines(spans: &[Inline], out: &mut Vec<StreamSpan>) {
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
fn restyle(out: &mut Vec<StreamSpan>, inner: &[Inline], style: SpanStyle) {
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
fn coalesce(spans: Vec<StreamSpan>) -> Vec<StreamSpan> {
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
pub(crate) fn flatten_nodes(block_id: u64, nodes: &[RenderNode]) -> Vec<StreamLine> {
    let mut lines = Vec::new();
    for node in nodes {
        flatten_node(block_id, node, 0, &mut lines);
    }
    lines
}

fn flatten_node(block_id: u64, node: &RenderNode, depth: usize, out: &mut Vec<StreamLine>) {
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
        RenderNode::CodeBlock { code, .. } => {
            // 本卡纯文本等宽渲染（T16 高亮由 T18 整合）：逐物理行一行，
            // 保留代码缩进；仅吞掉尾换行产生的末尾空行。
            let mut code_lines: Vec<&str> = code.split('\n').collect();
            if code_lines.last().is_some_and(|last| last.is_empty()) {
                code_lines.pop();
            }
            for code_line in code_lines {
                let mut line = StreamLine::new(block_id, LineKind::Code);
                line.depth = depth;
                line.spans.push(StreamSpan {
                    text: code_line.to_string(),
                    style: SpanStyle::Plain,
                });
                out.push(line);
            }
        }
        RenderNode::List(list) => flatten_list(block_id, list, depth, out),
        RenderNode::BlockQuote { children } => {
            let start = out.len();
            for child in children {
                flatten_node(block_id, child, depth, out);
            }
            for line in &mut out[start..] {
                line.kind = LineKind::Quote;
            }
        }
        RenderNode::Table(table) => flatten_table(block_id, table, out),
        RenderNode::ThematicBreak => out.push(StreamLine::new(block_id, LineKind::Rule)),
    }
}

fn flatten_list(block_id: u64, list: &ListBlock, depth: usize, out: &mut Vec<StreamLine>) {
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
                RenderNode::List(nested) => flatten_list(block_id, nested, depth + 1, out),
                other => flatten_node(block_id, other, depth, out),
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

fn flatten_table(block_id: u64, table: &TableBlock, out: &mut Vec<StreamLine>) {
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
fn inline_plain(spans: &[Inline]) -> String {
    fn push(spans: &[Inline], out: &mut String) {
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
struct CachedBlock {
    version: u64,
    lines: Vec<StreamLine>,
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
    committed_ids: Vec<u64>,
    /// Parallel to `committed_ids`: the materialized version of each block
    /// (invalidation detection without per-frame HashMap lookups).
    committed_versions: Vec<u64>,
    committed_lines: Vec<StreamLine>,
    pending_lines: Vec<StreamLine>,
    /// `(block_id, version)` of the pending rows currently materialized.
    pending_key: Option<(u64, u64)>,
    cache: std::collections::HashMap<u64, CachedBlock>,
}

impl StreamModel {
    /// Total row count (committed + pending).
    pub(crate) fn row_count(&self) -> usize {
        self.committed_lines.len() + self.pending_lines.len()
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
                .map(|pending| flatten_nodes(pending.block_id, pending.nodes))
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
    fn materialize_committed(
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
                let lines = flatten_nodes(block.block_id, block.nodes);
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
                let lines = flatten_nodes(block.block_id, block.nodes);
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

// ─── per-frame row rendering ─────────────────────────────────────────────────

/// Builds the visible rows for one `uniform_list` range by cloning cached
/// [`StreamLine`]s (no materialization here — that is the P3 contract).
pub(crate) fn build_rows(
    model: &StreamModel,
    range: Range<usize>,
    counters: &StreamCounters,
    cx: &App,
) -> Vec<AnyElement> {
    let row_t0 = Instant::now();
    let colors = theme(cx).colors;
    let rows: Vec<AnyElement> = range
        .filter_map(|index| model.row(index).map(|line| render_row(line, &colors)))
        .collect();
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
        .flex_shrink_0();
    match line.kind {
        LineKind::Code => {
            row = row
                .bg(colors.code_bg)
                .px_2()
                .font_family(MONOFONT.to_string())
                .text_size(px(Typography::CODE));
        }
        LineKind::Quote => {
            row = row.pl_2().text_color(colors.text_secondary).child(
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
                .px_2()
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
            row = row.pl_2().child(
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
    for span in &line.spans {
        row = row.child(render_span(span, colors));
    }
    row.into_any_element()
}

/// Heading tier → (font size token, weight token) — 字号全部取自 ui-spec §3
/// 的 Typography 档位，不发明新字号.
fn heading_style(level: u8) -> (f32, FontWeight) {
    match level {
        1..=2 => (Typography::HEADING_PAGE, Typography::HEADING_PAGE_WEIGHT),
        3..=4 => (Typography::HEADING_BLOCK, Typography::HEADING_BLOCK_WEIGHT),
        _ => (Typography::MESSAGE, Typography::HEADING_CARD_WEIGHT),
    }
}

fn render_span(span: &StreamSpan, colors: &ThemeColors) -> AnyElement {
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
    };
    text.into_any_element()
}

// ─── sample document + delta splitter (演示注入载荷) ─────────────────────────

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

/// Splits a document into 3..8-char deltas without splitting UTF-8 codepoints
/// (spike/T15 方法；演示注入与 bench 注入共用).
pub(crate) fn split_deltas(doc: &str, seed: u64) -> Vec<String> {
    let mut deltas = Vec::new();
    let mut chunk = String::new();
    let mut state = seed;
    for ch in doc.chars() {
        chunk.push(ch);
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        if chunk.chars().count() >= 3 + (state >> 33) as usize % 6 {
            deltas.push(std::mem::take(&mut chunk));
        }
    }
    if !chunk.is_empty() {
        deltas.push(chunk);
    }
    deltas
}

// ─── the conversation stream view ────────────────────────────────────────────

/// The opened-thread content view: thread header (title + anchor status +
/// demo-inject button) above the virtualized conversation stream. One entity
/// per open thread; rebuilt by the window root when another thread opens.
pub struct ConversationStream {
    thread: Thread,
    stream: MarkdownStream,
    model: StreamModel,
    counters: Arc<StreamCounters>,
    scroll: gpui::UniformListScrollHandle,
    anchor: anchor::AnchorState,
    /// Active demo injection (`None` = idle/finished).
    injecting: Option<InjectionState>,
}

struct InjectionState {
    deltas: Vec<String>,
    cursor: usize,
    /// When injection began (rate baseline; the 16ms tick only paces polling —
    /// the injected count follows 速率 × 已流时间 to absorb main-thread jitter).
    started: Instant,
}

impl ConversationStream {
    /// Builds the view for `thread` with an empty in-memory stream (S3 无消息
    /// 持久化：会话内容由流式注入产生，不落库).
    pub fn new(thread: Thread, _cx: &mut Context<Self>) -> Self {
        Self {
            thread,
            stream: MarkdownStream::new(),
            model: StreamModel::default(),
            counters: Arc::new(StreamCounters::default()),
            scroll: gpui::UniformListScrollHandle::new(),
            anchor: anchor::INITIAL,
            injecting: None,
        }
    }

    /// Starts the temporary demo injection (标题头旁按钮；S3 临时，T18 换
    /// mock 回放器): feeds the ~200-block sample at ~500 δ/s, then finishes
    /// the stream (tech-spec §5.4 终结语义).
    pub fn start_demo_injection(
        &mut self,
        _: &MouseUpEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.injecting.is_some() {
            return;
        }
        let deltas = split_deltas(&sample_document(200), 0x5EED);
        self.injecting = Some(InjectionState {
            deltas,
            cursor: 0,
            started: Instant::now(),
        });
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(INJECT_TICK).await;
                let alive = this
                    .update(cx, |this, cx| {
                        let Some(injection) = this.injecting.as_mut() else {
                            return false;
                        };
                        // 目标注入数 = ~500 δ/s × 已流时间（自校正）。
                        let target = (injection.started.elapsed().as_secs_f64()
                            * INJECT_RATE as f64) as usize;
                        let end = target.min(injection.deltas.len());
                        if injection.cursor < end {
                            for delta in &injection.deltas[injection.cursor..end] {
                                this.stream.append(delta);
                            }
                            injection.cursor = end;
                            cx.notify();
                        }
                        if end >= injection.deltas.len() {
                            this.injecting = None;
                            this.stream.finish();
                            cx.notify();
                            return false;
                        }
                        true
                    })
                    .unwrap_or(false);
                if !alive {
                    break;
                }
            }
        })
        .detach();
    }

    /// Scroll geometry snapshot: (distance to bottom, viewport height) in px.
    fn scroll_geometry(&self) -> (f32, f32) {
        let state = self.scroll.0.borrow();
        let base = &state.base_handle;
        let max_offset = f32::from(base.max_offset().y);
        let offset = f32::from(base.offset().y);
        let viewport = f32::from(base.bounds().size.height);
        ((max_offset + offset).max(0.0), viewport)
    }

    /// Renders the thread header: title + anchor status + demo button.
    fn render_header(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let title = if self.thread.title.is_empty() {
            "未命名任务".to_string()
        } else {
            self.thread.title.clone()
        };
        let following = self.anchor == anchor::AnchorState::Following;
        let (injected, total) = self
            .injecting
            .as_ref()
            .map(|injection| (injection.cursor, injection.deltas.len()))
            .unwrap_or((0, 0));
        div()
            .px(px(CONTENT_MIN_PADDING))
            .py(px(12.))
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .border_b_1()
            .border_color(colors.border_subtle)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(Typography::HEADING_PAGE))
                    .font_weight(Typography::HEADING_PAGE_WEIGHT)
                    .child(title),
            )
            .child(
                // 锚定状态指示（P4 走查辅助）。
                div()
                    .flex_shrink_0()
                    .text_size(px(Typography::SIDEBAR))
                    .text_color(colors.text_tertiary)
                    .child(if following {
                        "跟随中"
                    } else {
                        "已脱离 · 回到底部恢复"
                    }),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .text_size(px(Typography::SIDEBAR))
                    .text_color(colors.text_tertiary)
                    .child("S3 临时"),
            )
            .child(
                // 演示注入按钮（S3 临时，T18 换 mock 回放器）。
                div()
                    .flex_shrink_0()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .border_1()
                    .border_color(colors.border_subtle)
                    .bg(colors.bg_elevated)
                    .text_size(px(Typography::SIDEBAR))
                    .text_color(colors.text_secondary)
                    .cursor_pointer()
                    .hover(move |style| style.bg(colors.bg_hover))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::start_demo_injection))
                    .child(if injected > 0 {
                        format!("演示注入中 {injected}/{total} δ")
                    } else {
                        "演示注入".to_string()
                    }),
            )
            .into_any_element()
    }
}

impl Render for ConversationStream {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let render_t0 = Instant::now();
        let colors = theme(cx).colors;
        let counters = self.counters.clone();

        // 1) 差量同步：只有新/失效块被物化（P3）。
        let content_grew = {
            let snapshot = self.stream.snapshot();
            self.model.sync(&snapshot, &self.counters)
        };

        // 2) 锚定跟随（P4）：贴底自动跟随，上翻 >1 屏停止，回底恢复。
        let (distance, viewport) = self.scroll_geometry();
        let decision = anchor::step(self.anchor, distance, viewport, content_grew);
        self.anchor = decision.state;
        if decision.action == anchor::AnchorAction::StickToBottom {
            self.scroll.scroll_to_bottom();
        }

        let rows = self.model.row_count();
        let body: AnyElement = if rows == 0 {
            // §4.6 空态：内存态会话从演示注入开始。
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(colors.text_tertiary)
                .text_size(px(Typography::BODY))
                .child("会话内容为空：点右上「演示注入」以 ~500 δ/s 流式生成（S3 内存态）")
                .into_any_element()
        } else {
            div()
                .id("conversation-scroll")
                .size_full()
                .overflow_hidden()
                .child(
                    uniform_list(
                        "conversation-stream",
                        rows,
                        cx.processor(
                            move |this: &mut ConversationStream,
                                  range: Range<usize>,
                                  _window,
                                  cx| {
                                build_rows(&this.model, range, &this.counters, cx)
                            },
                        ),
                    )
                    .track_scroll(&self.scroll)
                    .h_full()
                    .w_full(),
                )
                .into_any_element()
        };

        let element = div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(colors.bg_base)
            .text_color(colors.text_primary)
            .child(self.render_header(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_hidden()
                    .child(body),
            )
            .into_any_element();
        counters.record_render(render_t0);
        element
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vega_markdown::{ListItem, TableCell};

    // ---------- 锚定状态机（P4） ----------

    use anchor::{AnchorAction as Action, AnchorState as State};

    fn step(state: State, distance: f32, viewport: f32, grew: bool) -> (State, Action) {
        let decision = anchor::step(state, distance, viewport, grew);
        (decision.state, decision.action)
    }

    #[test]
    fn following_at_bottom_sticks_on_new_content() {
        assert_eq!(
            step(State::Following, 0.0, 600.0, true),
            (State::Following, Action::StickToBottom)
        );
    }

    #[test]
    fn following_at_bottom_without_content_stays() {
        assert_eq!(
            step(State::Following, 0.0, 600.0, false),
            (State::Following, Action::StayPut)
        );
    }

    #[test]
    fn following_within_one_screen_still_jumps_on_content() {
        // 上翻半屏：仍贴底跟随（超过 1 屏才解除跟随）。
        assert_eq!(
            step(State::Following, 300.0, 600.0, true),
            (State::Following, Action::StickToBottom)
        );
    }

    #[test]
    fn following_beyond_one_screen_detaches_and_stays() {
        assert_eq!(
            step(State::Following, 700.0, 600.0, true),
            (State::Detached, Action::StayPut)
        );
        assert_eq!(
            step(State::Following, 700.0, 600.0, false),
            (State::Detached, Action::StayPut)
        );
    }

    #[test]
    fn detach_boundary_is_strictly_more_than_one_viewport() {
        assert_eq!(
            step(State::Following, 600.0, 600.0, true),
            (State::Following, Action::StickToBottom)
        );
        assert_eq!(
            step(State::Following, 600.5, 600.0, true),
            (State::Detached, Action::StayPut)
        );
    }

    #[test]
    fn detached_view_never_jumps_on_new_content() {
        // 脱离后新内容把距离越推越远，仍不跳。
        assert_eq!(
            step(State::Detached, 700.0, 600.0, true),
            (State::Detached, Action::StayPut)
        );
        assert_eq!(
            step(State::Detached, 1200.0, 600.0, true),
            (State::Detached, Action::StayPut)
        );
    }

    #[test]
    fn detached_resumes_when_back_at_bottom() {
        assert_eq!(
            step(State::Detached, 0.0, 600.0, false),
            (State::Following, Action::StickToBottom)
        );
    }

    #[test]
    fn epsilon_counts_as_bottom() {
        assert_eq!(
            step(State::Detached, 0.9, 600.0, false),
            (State::Following, Action::StickToBottom)
        );
        assert_eq!(
            step(State::Following, 1.0, 600.0, true),
            (State::Following, Action::StickToBottom)
        );
    }

    #[test]
    fn zero_viewport_disables_detach_rule() {
        // 首帧布局前 viewport=0：只跟随，不误判脱离。
        assert_eq!(
            step(State::Following, 500.0, 0.0, true),
            (State::Following, Action::StickToBottom)
        );
    }

    // ---------- RenderNode → 行映射（§5.3 关键分支） ----------

    fn spans_text(line: &StreamLine) -> String {
        line.spans.iter().map(|span| span.text.as_str()).collect()
    }

    #[test]
    fn table_maps_header_and_rows_with_padded_alignment() {
        let node = RenderNode::Table(TableBlock {
            alignments: vec![TableAlignment::Left, TableAlignment::Right],
            header: vec![
                TableCell {
                    spans: vec![Inline::Text("列A".into())],
                },
                TableCell {
                    spans: vec![Inline::Text("B".into())],
                },
            ],
            rows: vec![vec![
                TableCell {
                    spans: vec![Inline::Text("1".into())],
                },
                TableCell {
                    spans: vec![Inline::Text("数据".into())],
                },
            ]],
        });
        let lines = flatten_nodes(7, &[node]);
        // 表头一行 + 表体一行；两列 → cell+分隔+cell = 3 span。
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].kind, LineKind::TableHeader);
        assert_eq!(lines[0].spans.len(), 3);
        // 右对齐列按显示宽（CJK=2）补空格："列A" 宽 3 → "B" 前补 3 空格。
        assert_eq!(spans_text(&lines[0]), "列A │    B");
        assert_eq!(lines[1].kind, LineKind::TableRow);
        // "数据" 宽 4 使第 2 列宽为 4；"1" 左对齐补到 3 宽。
        assert_eq!(spans_text(&lines[1]), "1   │ 数据");
    }

    #[test]
    fn nested_lists_indent_and_number() {
        let node = RenderNode::List(ListBlock {
            ordered: false,
            start: 1,
            items: vec![
                ListItem {
                    checked: None,
                    children: vec![RenderNode::Paragraph {
                        spans: vec![Inline::Text("outer".into())],
                    }],
                },
                ListItem {
                    checked: Some(false),
                    children: vec![
                        RenderNode::Paragraph {
                            spans: vec![Inline::Text("task".into())],
                        },
                        RenderNode::List(ListBlock {
                            ordered: true,
                            start: 3,
                            items: vec![ListItem {
                                checked: None,
                                children: vec![RenderNode::Paragraph {
                                    spans: vec![Inline::Text("inner".into())],
                                }],
                            }],
                        }),
                    ],
                },
            ],
        });
        let lines = flatten_nodes(9, &[node]);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].kind, LineKind::ListItem);
        assert_eq!(lines[0].marker, "•");
        assert_eq!(lines[0].depth, 0);
        assert_eq!(lines[1].checked, Some(false));
        assert_eq!(lines[2].marker, "3.");
        assert_eq!(lines[2].depth, 1);
        assert_eq!(spans_text(&lines[2]), "inner");
    }

    #[test]
    fn code_block_splits_physical_lines_monospaced() {
        let node = RenderNode::CodeBlock {
            language: Some("rust".into()),
            code: "fn a() {\n    let x = 1;\n}\n".into(),
        };
        let lines = flatten_nodes(11, &[node]);
        // 尾换行不产生空行。
        assert_eq!(lines.len(), 3);
        assert!(lines.iter().all(|line| line.kind == LineKind::Code));
        assert_eq!(spans_text(&lines[1]), "    let x = 1;");
    }

    #[test]
    fn inline_styles_map_to_span_styles() {
        let node = RenderNode::Paragraph {
            spans: vec![
                Inline::Text("a ".into()),
                Inline::Strong(vec![Inline::Text("b".into())]),
                Inline::Text(" ".into()),
                Inline::Code("c".into()),
                Inline::Strikethrough(vec![Inline::Text("d".into())]),
                Inline::Link {
                    url: "https://example.com".into(),
                    title: None,
                    spans: vec![Inline::Text("e".into())],
                },
            ],
        };
        let lines = flatten_nodes(13, &[node]);
        assert_eq!(lines.len(), 1);
        let styles: Vec<SpanStyle> = lines[0].spans.iter().map(|span| span.style).collect();
        assert_eq!(
            styles,
            vec![
                SpanStyle::Plain,
                SpanStyle::Strong,
                SpanStyle::Plain,
                SpanStyle::Code,
                SpanStyle::Strikethrough,
                SpanStyle::Link,
            ]
        );
        assert_eq!(spans_text(&lines[0]), "a b cde");
    }

    #[test]
    fn blockquote_lines_get_quote_kind() {
        let node = RenderNode::BlockQuote {
            children: vec![RenderNode::Paragraph {
                spans: vec![Inline::Text("quoted".into())],
            }],
        };
        let lines = flatten_nodes(15, &[node]);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].kind, LineKind::Quote);
    }

    #[test]
    fn display_width_counts_cjk_as_two() {
        assert_eq!(display_width("ab"), 2);
        assert_eq!(display_width("中文"), 4);
        assert_eq!(display_width("中a文"), 5);
    }

    #[test]
    fn split_deltas_never_splits_codepoints_and_round_trips() {
        let doc = sample_document(3);
        let deltas = split_deltas(&doc, 0x5EED);
        assert!(deltas.len() > 10);
        assert_eq!(deltas.concat(), doc);
        assert!(deltas.iter().all(|delta| delta.chars().count() <= 8));
    }

    // ---------- StreamModel 差量渲染（P3 冻结契约） ----------

    fn stream_long_doc(blocks: usize) -> (MarkdownStream, usize) {
        let doc = sample_document(blocks);
        let deltas = split_deltas(&doc, 0x5EED);
        let mut stream = MarkdownStream::new();
        let total = deltas.len();
        for delta in &deltas {
            stream.append(delta);
        }
        (stream, total)
    }

    #[test]
    fn stream_model_freezes_committed_blocks_during_streaming() {
        let (mut stream, _deltas) = stream_long_doc(40);
        let mut model = StreamModel::default();
        let counters = StreamCounters::default();
        // 首轮同步：物化全部 committed 块。
        {
            let snapshot = stream.snapshot();
            model.sync(&snapshot, &counters);
        }
        let committed_after_first = counters.committed_materializations.load(Ordering::Relaxed);
        let rows_after_first = model.row_count();
        assert!(committed_after_first > 10);
        assert!(rows_after_first > 40);

        // 追加一段新内容：只物化新块，冻结块零重排（P3）。
        for delta in split_deltas("\n\n追加的**新**段落，含 `code`。\n\n", 7) {
            stream.append(&delta);
        }
        {
            let snapshot = stream.snapshot();
            model.sync(&snapshot, &counters);
        }
        assert_eq!(
            counters.frozen_rematerializations.load(Ordering::Relaxed),
            0,
            "frozen blocks must never re-materialize during streaming"
        );
        let committed_after_append = counters.committed_materializations.load(Ordering::Relaxed);
        assert!(
            committed_after_append > committed_after_first,
            "the new tail block must be materialized exactly once"
        );
        assert!(model.row_count() > rows_after_first);

        // 再次同步（内容未变）：不产生任何新物化。
        {
            let snapshot = stream.snapshot();
            let changed = model.sync(&snapshot, &counters);
            assert!(!changed);
        }
        assert_eq!(
            counters.committed_materializations.load(Ordering::Relaxed),
            committed_after_append
        );
        assert_eq!(
            counters.frozen_rematerializations.load(Ordering::Relaxed),
            0
        );
    }
}
