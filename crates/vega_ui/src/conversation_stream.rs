//! Virtualized conversation stream (S3-T18): message-block UI over a
//! [`vega_markdown::MarkdownStream`] — a `uniform_list` of uniform-height
//! rows with anchored tail-following, a Composer (multi-line input + send)
//! for local user echoes, and the S3 demo injection driven by the public
//! mock replayer ([`vega_markdown::MockReplay`]).
//!
//! Layering (tech-spec §5.1/§5.3 — the self-built parts of this card):
//!
//! ```text
//! MockReplay (mock 回放器，vega_markdown) ─▶ MarkdownStream.append(delta)
//!     ├─ committed blocks (BlockId stable, frozen) ─▶ StreamModel diff by
//!     │                                              (block_id, version):
//!     │                                              only new/invalidated
//!     │                                              blocks materialize
//!     │                                              into StreamLines
//!     │                                              (committed code blocks
//!     │                                              highlight via T16)
//!     └─ pending tail block                        ─▶ light re-flatten,
//!                                                    plain monospace
//! uniform_list(range) ─▶ per-frame rows built by cloning StreamLines
//!                        (frozen rows never re-materialize — P3)
//! ```
//!
//! - **消息块结构 (T18)**: the stream is a list of [`StreamEntry`]s — user
//!   messages (bg_elevated rounded card + 「你」 label, 独立渲染路径挂
//!   StreamSnapshot 外侧，架构师裁决) alternating with assistant turns (no
//!   card, direct markdown flow). One `MarkdownStream` per assistant turn.
//! - **差量渲染**: each assistant turn's [`StreamModel::sync`] materializes a
//!   committed block exactly once per `(block_id, version)`; frozen blocks
//!   keep their [`StreamLine`]s for the lifetime of the stream, so streaming
//!   appends only touch the tail (spike counter method: frozen
//!   re-materializations stay 0).
//! - **高亮整合 (T18)**: committed code blocks map [`HighlightSpan`] kinds
//!   onto the existing ui-spec §2 tokens (no new color values; the mapping
//!   table is [`code_token_style`]); pending/unclosed fences and unsupported
//!   languages degrade to plain monospace (tech-spec §5.1).
//! - **锚定跟随 (P4)**: pure state machine [`anchor::step`] — pinned at the
//!   bottom it follows new content; scrolling up more than one viewport
//!   detaches; returning to the bottom re-engages.
//! - **Composer (T18 最小版)**: fixed 3-row multi-line [`TextInput`] + send
//!   button; Cmd+Enter and the button share one submit handler; empty input
//!   disables send. A send appends a user entry (local echo, no LLM).
//! - **动效禁令 (tech-spec §5.4)**: streaming nodes get NO entrance
//!   animation — none is introduced anywhere in this pipeline.
//! - Rows are single logical lines at a fixed height (the `uniform_list`
//!   contract); long lines truncate. Block types map per ui-spec §3 tokens.
//!
//! The stream is memory-only (S3 has no message persistence): opening a
//! thread constructs empty entries; restarting clears the conversation.

pub mod bench;

use std::collections::HashMap;
use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, FontWeight, MouseButton,
    MouseUpEvent, Render, Rgba, Window, actions, div, px, uniform_list,
};
use vega_conversation::agent::PermissionQueue;
use vega_conversation::types::{
    ConversationEvent, ConversationMeter, MeterSnapshot, PermissionMode, Plan, RestoredUsage,
    RunUsageEstimator, TaskCostSummary, Thread, ThreadMode,
};
use vega_markdown::{
    BlockView, HighlightKind, HighlightSpan, Inline, ListBlock, MarkdownStream, MockReplay,
    RenderNode, StreamSnapshot, TableAlignment, TableBlock,
};
use vega_theme::{ThemeColors, Typography, theme};

use crate::artifact_card::ArtifactCard;
use crate::branch_selector::BranchSelector;
use crate::commit_panel::CommitPanel;
use crate::permission_card::{PermissionCard, PermissionCardResolved};
use crate::plan_card::{PlanCard, PlanReviewRequested};
use crate::settings::SettingsOpen;
use crate::sidebar::CONTENT_MIN_PADDING;
use crate::summary_card::SummaryCard;
use crate::text_input::TextInput;
use crate::tool_card::ToolCard;

actions!(
    vega_conversation_stream,
    [
        SendMessage,
        PreviousMessage,
        ActivateThreadSetting,
        OpenWorkspaceDiff
    ]
);

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

/// Composer visible rows (T18 最小版固定 3 行；ui-spec §4.4 的 1~8 行自适应
/// 高度后置，任务卡允许).
const COMPOSER_ROWS: usize = 1;

/// Typed settings request emitted upward; persistence remains in conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadSettingsRequested {
    pub thread_id: String,
    pub mode: Option<ThreadMode>,
    pub permission_mode: Option<PermissionMode>,
}

/// Composer submission routed to the application controller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposerSubmitted {
    pub thread_id: String,
    pub content: String,
}

/// Safe route request emitted by the thread header and Cmd+Shift+D.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenWorkspaceDiffRequested {
    pub thread_id: String,
    pub project_id: String,
}

#[derive(Clone, PartialEq, Eq)]
pub struct OpenCommitPanelRequested {
    pub thread_id: String,
    pub project_id: String,
}

impl std::fmt::Debug for OpenCommitPanelRequested {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OpenCommitPanelRequested")
            .field("thread_id_bytes", &self.thread_id.len())
            .field("project_id_bytes", &self.project_id.len())
            .finish()
    }
}

/// Content-free notification that a tool reached a terminal state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceToolTerminal {
    pub thread_id: String,
    pub project_id: String,
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

fn flatten_node(
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
fn code_line_spans(
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

fn flatten_list(
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
    fn row_count(&self, cx: &App) -> usize {
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
const USER_BLOCK_BASE: u64 = u64::MAX - (1 << 32);

/// Materializes a user echo block (T18 消息块结构): 「你」 label row, one card
/// line per source line (first/last flagged for rounding/border edges), and a
/// trailing spacer row separating it from the next message.
fn user_message_lines(block_id: u64, text: &str) -> Vec<StreamLine> {
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

/// The opened-thread content view: thread header (title + anchor status +
/// demo-inject button), the virtualized message stream, and the fixed-bottom
/// Composer. One entity per open thread; rebuilt by the window root when
/// another thread opens.
pub struct ConversationStream {
    thread: Thread,
    /// 消息块列表（T18）：user 回显与 assistant 流交替，顺序即会话顺序。
    entries: Vec<StreamEntry>,
    counters: Arc<StreamCounters>,
    scroll: gpui::UniformListScrollHandle,
    anchor: anchor::AnchorState,
    /// Active demo injection (`None` = idle/finished).
    injecting: Option<InjectionState>,
    /// Composer 输入状态（独立 `TextInput` Entity，固定 3 行多行）。
    input: Entity<TextInput>,
    /// Synthetic block-id counter for user echo rows (diagnostics only).
    user_block_seq: u64,
    /// Rows changed outside the assistant sync path (user send) — feeds the
    /// anchor's `content_grew` on the next frame.
    rows_dirty: bool,
    /// Opaque provider call ids are retained only as non-rendered map keys.
    tool_cards: HashMap<String, Entity<ToolCard>>,
    /// Exact call id to its sole inline artifact card.
    artifact_cards: HashMap<String, Entity<ArtifactCard>>,
    /// Route-owned safe branch selector; Git authority remains in the app controller.
    branch_selector: Entity<BranchSelector>,
    /// IO-free canonical commit panel; repository authority stays in app/headless.
    commit_panel: Entity<CommitPanel>,
    /// Concrete runtime permission hook shared by the owning conversation.
    permission_queue: PermissionQueue,
    /// The sole visible prompt; the opaque call id is only a map association.
    active_permission: Option<Entity<PermissionCard>>,
    /// Plan ids are opaque map keys; card content is a typed projection.
    plan_cards: HashMap<String, Entity<PlanCard>>,
    /// Sole applied per-task cost summary keyed by assistant message id
    /// (S7-T40); duplicate/later stale applications are ignored.
    summary_cards: HashMap<String, Entity<SummaryCard>>,
    /// Exact active durable assistant id and its stream-entry index.
    active_agent_message: Option<(String, usize)>,
    /// Most recently finished assistant entry, retained until the typed Plan
    /// projection can replace it in place.
    last_finished_agent_message: Option<(String, usize)>,
    /// Bounded token/cost meter projection (S7-T39/C3/C4). Pure shared
    /// `vega_conversation::types` state: no IO, no persistence; the Composer
    /// renders its snapshot and every update is checked arithmetic only.
    meter: ConversationMeter,
    /// Submitted drafts, scoped to this thread view.
    composer_history: Vec<String>,
    composer_submit_pending: bool,
    history_cursor: Option<usize>,
    history_draft: Option<String>,
    approved_not_started: bool,
    trusted_action_busy: bool,
    setting_focus: [FocusHandle; 6],
    controller_error: Option<String>,
    /// Cancels the watch listener and drops its fail-closed guard with the view.
    _permission_listener_task: gpui::Task<()>,
}

impl EventEmitter<PlanReviewRequested> for ConversationStream {}
impl EventEmitter<ThreadSettingsRequested> for ConversationStream {}
impl EventEmitter<ComposerSubmitted> for ConversationStream {}
impl EventEmitter<OpenWorkspaceDiffRequested> for ConversationStream {}
impl EventEmitter<OpenCommitPanelRequested> for ConversationStream {}
impl EventEmitter<WorkspaceToolTerminal> for ConversationStream {}

struct InjectionState {
    /// Which assistant entry the replayer feeds.
    entry_index: usize,
    /// The public mock replayer (vega_markdown::replay，T18 公共化).
    replay: MockReplay,
}

impl ConversationStream {
    /// Builds the view for `thread` with an empty in-memory stream (S3 无消息
    /// 持久化：会话内容由流式注入与 Composer 回显产生，不落库；重启后清空
    /// 是预期行为).
    pub fn new(thread: Thread, cx: &mut Context<Self>) -> Self {
        Self::new_with_permission_queue(thread, PermissionQueue::new(), cx)
    }

    /// Builds a stream with the exact permission queue passed to the runtime.
    pub fn new_with_permission_queue(
        thread: Thread,
        permission_queue: PermissionQueue,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            TextInput::new_multiline(
                cx,
                "输入消息…（Enter 换行 · Cmd+Enter 发送）",
                COMPOSER_ROWS,
            )
        });
        let branch_selector =
            cx.new(|cx| BranchSelector::new(thread.id.clone(), thread.project_id.clone(), cx));
        let commit_panel =
            cx.new(|cx| CommitPanel::new(thread.id.clone(), thread.project_id.clone(), cx));
        // 空输入禁用发送：输入内容变化即重渲染 Composer。
        cx.observe(&input, |_, _, cx| cx.notify()).detach();
        let mut listener = permission_queue.subscribe();
        let permission_listener_task = cx.spawn(async move |this, cx| {
            while listener.changed().await {
                let alive = this
                    .update(cx, |this, cx| this.install_pending_permission(cx))
                    .is_ok();
                if !alive {
                    break;
                }
            }
        });
        cx.observe_global::<SettingsOpen>(|this, cx| {
            if cx
                .try_global::<SettingsOpen>()
                .is_some_and(|settings| settings.0)
            {
                this.timeout_permission(cx);
            }
        })
        .detach();
        Self {
            thread,
            entries: Vec::new(),
            counters: Arc::new(StreamCounters::default()),
            scroll: gpui::UniformListScrollHandle::new(),
            anchor: anchor::INITIAL,
            injecting: None,
            input,
            user_block_seq: USER_BLOCK_BASE,
            rows_dirty: false,
            tool_cards: HashMap::new(),
            artifact_cards: HashMap::new(),
            branch_selector,
            commit_panel,
            permission_queue,
            active_permission: None,
            plan_cards: HashMap::new(),
            summary_cards: HashMap::new(),
            active_agent_message: None,
            last_finished_agent_message: None,
            meter: ConversationMeter::default(),
            composer_history: Vec::new(),
            composer_submit_pending: false,
            history_cursor: None,
            history_draft: None,
            approved_not_started: false,
            trusted_action_busy: false,
            setting_focus: [
                cx.focus_handle().tab_index(10).tab_stop(true),
                cx.focus_handle().tab_index(11).tab_stop(true),
                cx.focus_handle().tab_index(12).tab_stop(true),
                cx.focus_handle().tab_index(13).tab_stop(true),
                cx.focus_handle().tab_index(14).tab_stop(true),
                cx.focus_handle().tab_index(15).tab_stop(true),
            ],
            controller_error: None,
            _permission_listener_task: permission_listener_task,
        }
    }

    /// Applies the authoritative persisted thread settings after a request.
    pub fn apply_thread(&mut self, thread: Thread, cx: &mut Context<Self>) {
        if thread.id == self.thread.id && thread.project_id == self.thread.project_id {
            self.thread = thread;
            self.controller_error = None;
            cx.notify();
        }
    }

    /// Seeds thread-scoped Composer history from the typed conversation
    /// projection. This is called once when a stream entity is constructed.
    pub fn apply_composer_history(
        &mut self,
        thread_id: &str,
        history: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        if thread_id != self.thread.id || self.composer_submit_pending {
            return;
        }
        self.composer_history = history;
        self.history_cursor = None;
        self.history_draft = None;
        cx.notify();
    }

    /// Displays a bounded controller failure without changing authoritative
    /// selected state.
    pub fn apply_controller_error(&mut self, cx: &mut Context<Self>) {
        self.controller_error = Some("操作未保存，请重试".into());
        cx.notify();
    }

    /// Displays a bounded provider/runner failure after durable preparation.
    pub fn apply_agent_error(&mut self, cx: &mut Context<Self>) {
        self.controller_error = Some("执行未完成，可安全重试".into());
        // S7-T39: run-scoped estimate state never survives a failure path.
        self.meter.end_run();
        cx.notify();
    }

    /// Installs the frozen per-run provisional estimator (S7-T39/C3 run
    /// ownership). Called by the app exactly once per agent run, immediately
    /// after `agent_controller.begin`.
    pub fn install_meter_estimator(
        &mut self,
        estimator: Option<RunUsageEstimator>,
        cx: &mut Context<Self>,
    ) {
        self.meter.install_run_estimator(estimator);
        cx.notify();
    }

    /// Restores the calibrated counter baseline from the durable checked
    /// aggregate (S7-T39/C4 restart recovery). Called once when a stream
    /// entity is constructed for a thread with priced usage history.
    pub fn restore_meter(&mut self, usage: RestoredUsage, cx: &mut Context<Self>) {
        self.meter.restore(usage);
        cx.notify();
    }

    /// Current counter projection (C4): the Composer renders exactly this.
    pub fn meter_snapshot(&self) -> MeterSnapshot {
        self.meter.snapshot()
    }

    /// Feeds one conversation event through the meter projection, fenced by
    /// the same acceptance rules the stream applies (S7-T39 thread/run fence:
    /// late or stale-message events never calibrate or estimate).
    fn feed_meter(&mut self, event: &ConversationEvent, cx: &mut Context<Self>) {
        let accepted = match event {
            ConversationEvent::MessageStarted { .. } => self.active_agent_message.is_none(),
            ConversationEvent::TextDelta { message_id, .. }
            | ConversationEvent::UsageUpdated { message_id, .. } => self
                .active_agent_message
                .as_ref()
                .is_some_and(|(active_id, _)| active_id == message_id),
            ConversationEvent::MessageFinished { message_id, .. }
            | ConversationEvent::Interrupted { message_id } => self
                .active_agent_message
                .as_ref()
                .is_some_and(|(active_id, _)| active_id == message_id),
            ConversationEvent::Error { message_id, .. } => match message_id {
                Some(message_id) => self
                    .active_agent_message
                    .as_ref()
                    .is_some_and(|(active_id, _)| active_id == message_id),
                None => true,
            },
            // Tool proposals/results carry no assistant id; the meter gates
            // them on its own run state. Thinking is never visible output.
            ConversationEvent::ToolCallProposed { .. }
            | ConversationEvent::ToolCallApproved { .. }
            | ConversationEvent::ToolCallOutput { .. }
            | ConversationEvent::ToolCallFinished { .. } => true,
            ConversationEvent::ThinkingDelta { .. } => false,
        };
        if accepted && self.meter.apply(event) {
            cx.notify();
        }
    }

    /// Shows a restart-safe, non-executing projection for a durable approved
    /// instruction. Merely opening a thread must never read Keychain or start
    /// a provider request.
    pub fn apply_approved_not_started(&mut self, cx: &mut Context<Self>) {
        self.approved_not_started = true;
        self.controller_error = Some("已批准计划尚未执行；恢复入口待补充".into());
        cx.notify();
    }

    /// Adds or refreshes a validated durable Plan without direct SQLite UI access.
    pub fn apply_plan(&mut self, plan: Plan, cx: &mut Context<Self>) {
        if plan.thread_id != self.thread.id {
            return;
        }
        if let Some(card) = self.plan_cards.get(&plan.id) {
            card.update(cx, |card, cx| card.apply_persisted(plan, cx));
            return;
        }
        let id = plan.id.clone();
        let card = cx.new(|cx| PlanCard::new(plan, cx));
        cx.subscribe(&card, |this, _, event: &PlanReviewRequested, cx| {
            cx.emit(event.clone());
            this.rows_dirty = true;
        })
        .detach();
        cx.observe(&card, |this, _, cx| {
            this.rows_dirty = true;
            cx.notify();
        })
        .detach();
        let replace_index = self
            .last_finished_agent_message
            .as_ref()
            .filter(|(message_id, _)| message_id == &id)
            .map(|(_, entry_index)| *entry_index);
        if let Some(entry_index) = replace_index {
            self.last_finished_agent_message = None;
            if let Some(entry) = self.entries.get_mut(entry_index) {
                *entry = StreamEntry::Plan { card: card.clone() };
            } else {
                self.entries.push(StreamEntry::Plan { card: card.clone() });
            }
        } else {
            self.entries.push(StreamEntry::Plan { card: card.clone() });
        }
        self.plan_cards.insert(id, card);
        self.rows_dirty = true;
        cx.notify();
    }

    /// Appends the read-only per-task cost summary card of one finished
    /// assistant message (S7-T40/C4). The typed projection arrives from the
    /// app layer via `vega_conversation::summary`; the stream never queries
    /// SQLite and never computes a cost formula. Applications are keyed by
    /// the assistant message id and first-wins: duplicates and later stale
    /// projections of the same task are ignored, and projections of foreign
    /// threads are dropped.
    pub fn apply_task_summary(&mut self, summary: TaskCostSummary, cx: &mut Context<Self>) {
        if self.summary_cards.contains_key(&summary.message_id) {
            return;
        }
        let message_id = summary.message_id.clone();
        let card = cx.new(|_| SummaryCard::new(summary));
        self.entries
            .push(StreamEntry::Summary { card: card.clone() });
        self.summary_cards.insert(message_id, card);
        self.rows_dirty = true;
        cx.notify();
    }

    /// Hook passed to the conversation runner for this visible stream.
    pub fn permission_queue(&self) -> PermissionQueue {
        self.permission_queue.clone()
    }

    pub fn branch_selector(&self) -> Entity<BranchSelector> {
        self.branch_selector.clone()
    }

    pub fn commit_panel(&self) -> Entity<CommitPanel> {
        self.commit_panel.clone()
    }

    /// Content-free app-controller guards for trusted workspace actions.
    pub fn has_active_agent(&self) -> bool {
        self.active_agent_message.is_some()
    }

    pub fn has_pending_permission(&self) -> bool {
        self.permission_queue.has_pending()
    }

    pub fn has_pending_plan_review(&self, cx: &App) -> bool {
        self.plan_cards
            .values()
            .any(|card| card.read(cx).status() == vega_conversation::types::PlanStatus::Pending)
    }

    pub fn set_trusted_action_busy(&mut self, busy: bool, cx: &mut Context<Self>) {
        self.trusted_action_busy = busy;
        self.branch_selector
            .update(cx, |selector, cx| selector.set_disabled(busy, cx));
        self.commit_panel.update(cx, |panel, cx| {
            panel.set_disabled(busy && !panel.is_open(), cx)
        });
        cx.notify();
    }

    /// Fails the visible/pending prompt closed before Settings, thread switch,
    /// or window teardown hides the card.
    pub fn timeout_permission(&mut self, cx: &mut Context<Self>) {
        self.permission_queue.timeout_active();
        self.remove_active_permission(cx);
    }

    fn install_pending_permission(&mut self, cx: &mut Context<Self>) {
        let Some(pending) = self.permission_queue.take_pending() else {
            return;
        };
        if cx
            .try_global::<SettingsOpen>()
            .is_some_and(|settings| settings.0)
        {
            drop(pending);
            return;
        }
        let Some(request) = pending.request() else {
            drop(pending);
            return;
        };
        let call_id = request.call_id.clone();
        let identity_matches = self.tool_cards.get(&call_id).is_some_and(|card| {
            card.read(cx)
                .permission_identity()
                .is_some_and(|(tool, target)| {
                    tool == request.tool && target == request.display_target
                })
        });
        if !identity_matches {
            drop(pending);
            if let Some(card) = self.tool_cards.get(&call_id) {
                card.update(cx, ToolCard::fail_corrupt);
            } else {
                self.push_corrupt_tool(call_id, cx);
            }
            return;
        }
        self.remove_active_permission(cx);
        let Some((request, lease)) = pending.into_parts() else {
            return;
        };
        let card = cx.new(|cx| PermissionCard::new(&request, lease, cx));
        cx.subscribe(&card, |this, card, _: &PermissionCardResolved, cx| {
            if this
                .active_permission
                .as_ref()
                .is_some_and(|active| active == &card)
            {
                this.remove_active_permission(cx);
            }
        })
        .detach();
        self.entries
            .push(StreamEntry::Permission { card: card.clone() });
        self.active_permission = Some(card);
        self.anchor = anchor::AnchorState::Following;
        self.scroll.scroll_to_bottom();
        self.rows_dirty = true;
        cx.notify();
    }

    fn remove_active_permission(&mut self, cx: &mut Context<Self>) {
        let Some(active) = self.active_permission.take() else {
            return;
        };
        self.entries
            .retain(|entry| !matches!(entry, StreamEntry::Permission { card } if card == &active));
        self.rows_dirty = true;
        cx.notify();
    }

    /// Total row count across all entries.
    fn total_rows(&self, cx: &App) -> usize {
        self.entries.iter().map(|entry| entry.row_count(cx)).sum()
    }

    /// Applies an already-durable shared lifecycle event. The UI never reads
    /// SQLite and never consumes runtime-local events.
    pub fn apply_event(&mut self, event: ConversationEvent, cx: &mut Context<Self>) {
        // S7-T39: the bounded meter projection consumes the same accepted
        // events (fence below) before ownership moves into the render path.
        self.feed_meter(&event, cx);
        match event {
            ConversationEvent::MessageStarted { message_id, .. } => {
                if self.active_agent_message.is_some() {
                    self.apply_controller_error(cx);
                    return;
                }
                self.last_finished_agent_message = None;
                let entry_index = self.entries.len();
                self.entries.push(StreamEntry::Assistant {
                    stream: Box::new(MarkdownStream::new()),
                    model: StreamModel::default(),
                });
                self.active_agent_message = Some((message_id, entry_index));
                self.rows_dirty = true;
                cx.notify();
            }
            ConversationEvent::TextDelta { message_id, delta } => {
                let Some((active_id, entry_index)) = self.active_agent_message.as_ref() else {
                    return;
                };
                if active_id != &message_id {
                    return;
                }
                if let Some(StreamEntry::Assistant { stream, .. }) =
                    self.entries.get_mut(*entry_index)
                {
                    stream.append(&delta);
                    self.rows_dirty = true;
                    cx.notify();
                }
            }
            ConversationEvent::ThinkingDelta { .. } | ConversationEvent::UsageUpdated { .. } => {}
            ConversationEvent::ToolCallProposed { call } => {
                if let Some(existing) = self.tool_cards.get(&call.id) {
                    existing.update(cx, |card, cx| {
                        if !card.matches_call(&call) {
                            card.fail_corrupt(cx);
                        }
                    });
                    return;
                }
                let call_id = call.id.clone();
                let card = cx.new(|_| ToolCard::proposed(&call));
                cx.observe(&card, |this, _, cx| {
                    this.rows_dirty = true;
                    cx.notify();
                })
                .detach();
                self.entries.push(StreamEntry::Tool { card: card.clone() });
                self.tool_cards.insert(call_id, card);
                self.rows_dirty = true;
                cx.notify();
            }
            ConversationEvent::ToolCallApproved { call_id, approval } => {
                if let Some(card) = self.tool_cards.get(&call_id) {
                    card.update(cx, |card, cx| {
                        card.apply_approved(approval);
                        cx.notify();
                    });
                } else {
                    self.push_corrupt_tool(call_id, cx);
                }
            }
            ConversationEvent::ToolCallOutput { .. } => {
                // T26 emits a post-commit bounded output immediately before
                // Finished. Ignore it here: write/edit chunks can contain the
                // strict success JSON (including the opaque checkpoint ref),
                // and terminal projection is the sole card decode boundary.
            }
            ConversationEvent::ToolCallFinished { call_id, result } => {
                if self.active_permission.is_some() {
                    self.timeout_permission(cx);
                }
                if let Some(card) = self.tool_cards.get(&call_id) {
                    card.update(cx, |card, cx| {
                        card.apply_finished(&result);
                        cx.notify();
                    });
                } else {
                    let card = if result.invalid.is_some() {
                        ToolCard::invalid_terminal(&result)
                    } else {
                        ToolCard::corrupt()
                    };
                    self.push_tool_card(call_id, card, cx);
                }
                cx.emit(WorkspaceToolTerminal {
                    thread_id: self.thread.id.clone(),
                    project_id: self.thread.project_id.clone(),
                });
            }
            ConversationEvent::MessageFinished { message_id, .. }
            | ConversationEvent::Interrupted { message_id } => {
                self.finish_agent_message(&message_id, cx);
            }
            ConversationEvent::Error { message_id, .. } => {
                if let Some(message_id) = message_id {
                    self.finish_agent_message(&message_id, cx);
                } else {
                    self.timeout_permission(cx);
                }
            }
        }
    }

    fn finish_agent_message(&mut self, message_id: &str, cx: &mut Context<Self>) {
        let Some((active_id, entry_index)) = self.active_agent_message.as_ref() else {
            return;
        };
        if active_id != message_id {
            return;
        }
        if let Some(StreamEntry::Assistant { stream, .. }) = self.entries.get_mut(*entry_index) {
            stream.finish();
        }
        self.last_finished_agent_message = self.active_agent_message.take();
        self.timeout_permission(cx);
        self.rows_dirty = true;
        cx.notify();
    }

    fn push_corrupt_tool(&mut self, call_id: String, cx: &mut Context<Self>) {
        self.push_tool_card(call_id, ToolCard::corrupt(), cx);
    }

    fn push_tool_card(&mut self, call_id: String, card: ToolCard, cx: &mut Context<Self>) {
        if self.tool_cards.contains_key(&call_id) {
            return;
        }
        let card = cx.new(|_| card);
        cx.observe(&card, |this, _, cx| {
            this.rows_dirty = true;
            cx.notify();
        })
        .detach();
        self.entries.push(StreamEntry::Tool { card: card.clone() });
        self.tool_cards.insert(call_id, card);
        self.rows_dirty = true;
        cx.notify();
    }

    /// Inserts one artifact immediately after the exact tool entry. Identical
    /// duplicates reconcile in place; conflicting ids fail the existing card
    /// closed and never insert an unrelated entry.
    pub fn apply_artifact_card(
        &mut self,
        call_id: &str,
        card: Entity<ArtifactCard>,
        cx: &mut Context<Self>,
    ) -> bool {
        if let Some(existing) = self.artifact_cards.get(call_id) {
            let incoming = card.read(cx).projection().clone();
            if existing.read(cx).id() == incoming.id {
                existing.update(cx, |card, cx| {
                    let _ = card.apply_metadata(incoming, cx);
                });
                return true;
            }
            existing.update(cx, ArtifactCard::fail_corrupt);
            return false;
        }
        let Some(tool) = self.tool_cards.get(call_id) else {
            return false;
        };
        let Some(index) = self
            .entries
            .iter()
            .position(|entry| matches!(entry, StreamEntry::Tool { card } if card == tool))
        else {
            return false;
        };
        if matches!(
            self.entries.get(index + 1),
            Some(StreamEntry::Artifact { .. })
        ) {
            return false;
        }
        cx.observe(&card, |this, _, cx| {
            this.rows_dirty = true;
            cx.notify();
        })
        .detach();
        self.entries
            .insert(index + 1, StreamEntry::Artifact { card: card.clone() });
        self.artifact_cards.insert(call_id.to_owned(), card);
        self.rows_dirty = true;
        cx.notify();
        true
    }

    /// Content-free structural check used by the application integration
    /// harness to prove exact Tool -> Artifact adjacency.
    #[doc(hidden)]
    pub fn artifact_card_is_adjacent(&self, call_id: &str) -> bool {
        let Some(tool) = self.tool_cards.get(call_id) else {
            return false;
        };
        let Some(artifact) = self.artifact_cards.get(call_id) else {
            return false;
        };
        self.entries.windows(2).any(|entries| {
            matches!(&entries[0], StreamEntry::Tool { card } if card == tool)
                && matches!(&entries[1], StreamEntry::Artifact { card } if card == artifact)
        })
    }

    /// Content-free virtual-row count for integration tests of dynamic cards.
    #[doc(hidden)]
    pub fn virtual_row_count(&self, cx: &App) -> usize {
        self.total_rows(cx)
    }

    /// Starts the demo injection (标题头旁按钮)：drives the built-in
    /// ~200-block sample through the public mock replayer
    /// ([`vega_markdown::MockReplay`], 位于 vega_markdown) at ~500 δ/s into a
    /// fresh assistant entry, then finishes that stream (tech-spec §5.4 final
    /// 终结语义：作废 pending 补全残留).
    pub fn start_demo_injection(
        &mut self,
        _: &MouseUpEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.injecting.is_some() {
            return;
        }
        let entry_index = self.entries.len();
        self.entries.push(StreamEntry::Assistant {
            stream: Box::new(MarkdownStream::new()),
            model: StreamModel::default(),
        });
        self.injecting = Some(InjectionState {
            entry_index,
            replay: MockReplay::new(&sample_document(200), INJECT_RATE, 0x5EED),
        });
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(INJECT_TICK).await;
                let alive = this
                    .update(cx, |this, cx| {
                        let Some(injection) = this.injecting.as_mut() else {
                            return false;
                        };
                        // 回放器按「速率 × 已流时间」自校正出批（16ms tick
                        // 只负责轮询，主线程抖动不累积漂移）。
                        let batch = injection.replay.take_due();
                        let entry = this.entries.get_mut(injection.entry_index);
                        if let Some(StreamEntry::Assistant { stream, .. }) = entry {
                            for delta in &batch {
                                stream.append(delta);
                            }
                        }
                        if injection.replay.is_finished() {
                            // 回放结束 → final 终结语义（tech-spec §5.4）。
                            if let Some(StreamEntry::Assistant { stream, .. }) =
                                this.entries.get_mut(injection.entry_index)
                            {
                                stream.finish();
                            }
                            this.injecting = None;
                            cx.notify();
                            return false;
                        }
                        if !batch.is_empty() {
                            cx.notify();
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

    /// Requests a durable turn. Draft/history/user echo remain untouched until
    /// the controller observes durable `MessageStarted` for this exact run.
    fn submit_message(&mut self, cx: &mut Context<Self>) {
        if self.composer_submit_pending || self.approved_not_started || self.trusted_action_busy {
            return;
        }
        let text = self.input.read(cx).text().to_string();
        if text.is_empty() {
            return;
        }
        self.composer_submit_pending = true;
        cx.emit(ComposerSubmitted {
            thread_id: self.thread.id.clone(),
            content: text,
        });
        cx.notify();
    }

    /// Commits the local echo only after conversation persistence emitted the
    /// matching durable assistant start. Edits made while waiting are kept as
    /// the next draft rather than being cleared accidentally.
    pub fn accept_composer_submission(&mut self, content: &str, cx: &mut Context<Self>) {
        if !self.composer_submit_pending {
            return;
        }
        self.composer_submit_pending = false;
        if self.composer_history.last().map(String::as_str) != Some(content) {
            self.composer_history.push(content.to_string());
        }
        self.history_cursor = None;
        self.history_draft = None;
        if self.input.read(cx).text() == content {
            self.input.update(cx, TextInput::clear);
        }
        let block_id = self.user_block_seq;
        self.user_block_seq += 1;
        self.entries.push(StreamEntry::User {
            lines: user_message_lines(block_id, content),
        });
        self.rows_dirty = true;
        cx.notify();
    }

    /// Re-arms submit after preparation failed before any durable message.
    pub fn reject_composer_submission(&mut self, cx: &mut Context<Self>) {
        if self.composer_submit_pending {
            self.composer_submit_pending = false;
            cx.notify();
        }
    }

    /// Cmd+Enter in the Composer context ([`SendMessage`] binding).
    fn on_send_action(&mut self, _: &SendMessage, _: &mut Window, cx: &mut Context<Self>) {
        self.submit_message(cx);
    }

    /// [发送] button click — same submit path as Cmd+Enter.
    fn on_send_clicked(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.submit_message(cx);
    }

    /// Recalls older submitted drafts only from the first logical line; Up
    /// inside a multi-line draft remains a caret/navigation concern.
    fn on_previous_message(&mut self, _: &PreviousMessage, _: &mut Window, cx: &mut Context<Self>) {
        if self.composer_history.is_empty()
            || (self.history_cursor.is_none() && !self.input.read(cx).cursor_allows_history())
        {
            return;
        }
        let current = self.input.read(cx).text().to_string();
        if let Some(index) = self.history_cursor
            && self.composer_history.get(index) != Some(&current)
        {
            self.history_cursor = None;
            self.history_draft = Some(current.clone());
        }
        let index = match self.history_cursor {
            Some(index) => index.saturating_sub(1),
            None => {
                if self.history_draft.is_none() {
                    self.history_draft = Some(current);
                }
                self.composer_history.len() - 1
            }
        };
        self.history_cursor = Some(index);
        let recalled = self.composer_history[index].clone();
        self.input
            .update(cx, |input, cx| input.set_text(&recalled, cx));
    }

    fn request_mode(&mut self, mode: ThreadMode, cx: &mut Context<Self>) {
        if mode != self.thread.mode {
            cx.emit(ThreadSettingsRequested {
                thread_id: self.thread.id.clone(),
                mode: Some(mode),
                permission_mode: None,
            });
        }
    }

    fn select_ask(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.request_mode(ThreadMode::Ask, cx);
    }

    fn activate_ask(&mut self, _: &ActivateThreadSetting, _: &mut Window, cx: &mut Context<Self>) {
        self.request_mode(ThreadMode::Ask, cx);
    }

    fn select_plan(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.request_mode(ThreadMode::Plan, cx);
    }

    fn activate_plan(&mut self, _: &ActivateThreadSetting, _: &mut Window, cx: &mut Context<Self>) {
        self.request_mode(ThreadMode::Plan, cx);
    }

    fn select_execute(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.request_mode(ThreadMode::Execute, cx);
    }

    fn activate_execute(
        &mut self,
        _: &ActivateThreadSetting,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_mode(ThreadMode::Execute, cx);
    }

    fn request_permission_mode(&mut self, mode: PermissionMode, cx: &mut Context<Self>) {
        if mode != self.thread.permission_mode {
            cx.emit(ThreadSettingsRequested {
                thread_id: self.thread.id.clone(),
                mode: None,
                permission_mode: Some(mode),
            });
        }
    }

    fn select_readonly(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.request_permission_mode(PermissionMode::ReadOnly, cx);
    }

    fn activate_readonly(
        &mut self,
        _: &ActivateThreadSetting,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_permission_mode(PermissionMode::ReadOnly, cx);
    }

    fn select_confirm(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.request_permission_mode(PermissionMode::Confirm, cx);
    }

    fn activate_confirm(
        &mut self,
        _: &ActivateThreadSetting,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_permission_mode(PermissionMode::Confirm, cx);
    }

    fn select_auto(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.request_permission_mode(PermissionMode::Auto, cx);
    }

    fn activate_auto(&mut self, _: &ActivateThreadSetting, _: &mut Window, cx: &mut Context<Self>) {
        self.request_permission_mode(PermissionMode::Auto, cx);
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

    fn emit_open_diff(&mut self, cx: &mut Context<Self>) {
        cx.emit(OpenWorkspaceDiffRequested {
            thread_id: self.thread.id.clone(),
            project_id: self.thread.project_id.clone(),
        });
    }

    fn open_diff_clicked(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.emit_open_diff(cx);
    }

    fn open_diff_action(&mut self, _: &OpenWorkspaceDiff, _: &mut Window, cx: &mut Context<Self>) {
        self.emit_open_diff(cx);
    }

    fn open_commit_clicked(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.trusted_action_busy {
            return;
        }
        cx.emit(OpenCommitPanelRequested {
            thread_id: self.thread.id.clone(),
            project_id: self.thread.project_id.clone(),
        });
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
            .map(|injection| (injection.replay.injected(), injection.replay.total()))
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
                    .child("S3 演示"),
            )
            .child(
                // 演示注入按钮（驱动 vega_markdown::MockReplay 公共回放器）。
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
                    .when(!self.trusted_action_busy, |button| button.cursor_pointer())
                    .hover(move |style| style.bg(colors.bg_hover))
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::start_demo_injection))
                    .child(if injected > 0 {
                        format!("演示注入中 {injected}/{total} δ")
                    } else {
                        "演示注入".to_string()
                    }),
            )
            .child(
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
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::open_diff_clicked))
                    .child("Diff"),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .border_1()
                    .border_color(colors.border_subtle)
                    .bg(colors.bg_elevated)
                    .text_size(px(Typography::SIDEBAR))
                    .text_color(if self.trusted_action_busy {
                        colors.text_tertiary
                    } else {
                        colors.text_secondary
                    })
                    .when(!self.trusted_action_busy, |button| {
                        button
                            .cursor_pointer()
                            .hover(move |style| style.bg(colors.bg_hover))
                    })
                    .on_mouse_up(MouseButton::Left, cx.listener(Self::open_commit_clicked))
                    .child("Commit"),
            )
            .into_any_element()
    }

    /// Renders the Composer (T18 最小版，ui-spec §4.4 最小集 + §1 Composer 行
    /// 规格)：底部固定、圆角 12px（rounded_xl）、1px border_subtle、
    /// bg_elevated；固定 3 行多行输入 + [发送] 按钮（空输入禁用）。
    /// @引用/命令/模型选择器为 Composer 完全体范围，后置。
    fn render_composer(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        let can_send = !self.input.read(cx).text().is_empty()
            && !self.composer_submit_pending
            && !self.approved_not_started
            && !self.trusted_action_busy;
        div()
            .px(px(CONTENT_MIN_PADDING))
            .pt(px(8.))
            .pb(px(12.))
            .border_t_1()
            .border_color(colors.border_subtle)
            .child(
                div()
                    .w_full()
                    // Cmd+Enter 的按键上下文（绑定见 vega_ui::init）。
                    .key_context("Composer")
                    .on_action(cx.listener(Self::on_send_action))
                    .on_action(cx.listener(Self::on_previous_message))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .bg(colors.bg_elevated)
                    .border_1()
                    .border_color(colors.border_subtle)
                    .rounded_xl()
                    .p_2()
                    .overflow_hidden()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_2()
                            .child(self.render_mode_controls(cx))
                            .child(self.render_permission_controls(cx))
                            .child(self.branch_selector.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_end()
                            .gap_2()
                            .child(self.input.clone())
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .px_3()
                                    .py_1()
                                    .rounded_md()
                                    .text_size(px(Typography::SIDEBAR))
                                    .when(can_send, |button| {
                                        button
                                            .bg(colors.accent)
                                            .text_color(colors.bg_base)
                                            .cursor_pointer()
                                            .on_mouse_up(
                                                MouseButton::Left,
                                                cx.listener(Self::on_send_clicked),
                                            )
                                    })
                                    .when(!can_send, |button| {
                                        button.bg(colors.bg_hover).text_color(colors.text_tertiary)
                                    })
                                    .child("发送"),
                            ),
                    )
                    // ui-spec §4.4 token 计数器：右下角常驻 compact counter
                    // （S7-T39/C4）。数据只来自 conversation meter 投影；
                    // 更新路径零 IO（checked 整数运算），数字宽度变化只影响
                    // 本行文本，不触碰已冻结会话区（P3 不回退）。
                    .child(
                        div()
                            .flex()
                            .w_full()
                            .justify_end()
                            .text_size(px(Typography::SIDEBAR))
                            .text_color(colors.text_tertiary)
                            .child(self.meter.snapshot().display()),
                    ),
            )
            .children(self.controller_error.clone().map(|error| {
                div()
                    .mt_1()
                    .text_size(px(Typography::SIDEBAR))
                    .text_color(colors.danger)
                    .child(error)
            }))
            .into_any_element()
    }

    fn render_mode_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        div()
            .flex()
            .flex_row()
            .rounded_md()
            .border_1()
            .border_color(colors.border_subtle)
            .child(
                segment(
                    "Ask",
                    self.thread.mode == ThreadMode::Ask,
                    colors,
                    self.setting_focus[0].clone(),
                )
                .key_context("ThreadSettings")
                .on_action(cx.listener(Self::activate_ask))
                .on_mouse_up(MouseButton::Left, cx.listener(Self::select_ask)),
            )
            .child(
                segment(
                    "Plan",
                    self.thread.mode == ThreadMode::Plan,
                    colors,
                    self.setting_focus[1].clone(),
                )
                .key_context("ThreadSettings")
                .on_action(cx.listener(Self::activate_plan))
                .on_mouse_up(MouseButton::Left, cx.listener(Self::select_plan)),
            )
            .child(
                segment(
                    "Execute",
                    self.thread.mode == ThreadMode::Execute,
                    colors,
                    self.setting_focus[2].clone(),
                )
                .key_context("ThreadSettings")
                .on_action(cx.listener(Self::activate_execute))
                .on_mouse_up(MouseButton::Left, cx.listener(Self::select_execute)),
            )
            .into_any_element()
    }

    fn render_permission_controls(&self, cx: &mut Context<Self>) -> AnyElement {
        let colors = theme(cx).colors;
        div()
            .flex()
            .flex_row()
            .rounded_md()
            .border_1()
            .border_color(colors.border_subtle)
            .child(
                segment(
                    "ReadOnly",
                    self.thread.permission_mode == PermissionMode::ReadOnly,
                    colors,
                    self.setting_focus[3].clone(),
                )
                .key_context("ThreadSettings")
                .on_action(cx.listener(Self::activate_readonly))
                .on_mouse_up(MouseButton::Left, cx.listener(Self::select_readonly)),
            )
            .child(
                segment(
                    "Confirm",
                    self.thread.permission_mode == PermissionMode::Confirm,
                    colors,
                    self.setting_focus[4].clone(),
                )
                .key_context("ThreadSettings")
                .on_action(cx.listener(Self::activate_confirm))
                .on_mouse_up(MouseButton::Left, cx.listener(Self::select_confirm)),
            )
            .child(
                segment(
                    "Auto",
                    self.thread.permission_mode == PermissionMode::Auto,
                    colors,
                    self.setting_focus[5].clone(),
                )
                .key_context("ThreadSettings")
                .on_action(cx.listener(Self::activate_auto))
                .on_mouse_up(MouseButton::Left, cx.listener(Self::select_auto)),
            )
            .into_any_element()
    }
}

fn segment(
    label: &'static str,
    selected: bool,
    colors: ThemeColors,
    focus: FocusHandle,
) -> gpui::Div {
    div()
        .track_focus(&focus)
        .px_2()
        .py_1()
        .text_size(px(Typography::SIDEBAR))
        .cursor_pointer()
        .when(selected, |item| {
            item.bg(colors.bg_active).text_color(colors.text_primary)
        })
        .when(!selected, |item| item.text_color(colors.text_secondary))
        .child(label)
}

impl Render for ConversationStream {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let render_t0 = Instant::now();
        let colors = theme(cx).colors;
        let counters = self.counters.clone();

        // 1) 差量同步：每个 assistant 段只有新/失效块被物化（P3）；user 回显
        //    的行数变化经 rows_dirty 参与锚定判定。
        let mut content_grew = self.rows_dirty;
        self.rows_dirty = false;
        for entry in &mut self.entries {
            if let StreamEntry::Assistant { stream, model } = entry {
                let snapshot = stream.snapshot();
                content_grew |= model.sync(&snapshot, &self.counters);
            }
        }

        // 2) 锚定跟随（P4）：贴底自动跟随，上翻 >1 屏停止，回底恢复。
        let (distance, viewport) = self.scroll_geometry();
        let decision = anchor::step(self.anchor, distance, viewport, content_grew);
        self.anchor = decision.state;
        if decision.action == anchor::AnchorAction::StickToBottom {
            self.scroll.scroll_to_bottom();
        }

        let rows = self.total_rows(cx);
        let body: AnyElement = if rows == 0 {
            // §4.6 空态：内存态会话从演示注入或 Composer 开始。
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(colors.text_tertiary)
                .text_size(px(Typography::BODY))
                .child("会话内容为空：点右上「演示注入」以 ~500 δ/s 流式生成，或在下方输入后发送（S3 内存态）")
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
                                  window,
                                  cx| {
                                build_entry_rows(&this.entries, range, &this.counters, window, cx)
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
            .relative()
            .bg(colors.bg_base)
            .text_color(colors.text_primary)
            .key_context("ConversationStream")
            .on_action(cx.listener(Self::open_diff_action))
            // tech-spec §5.4 动效禁令：流式期间节点无任何入场 opacity/动画
            // （本管线自 T17 起即不引入入场动画，T18 维持）。
            .child(self.render_header(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_hidden()
                    .child(body),
            )
            .child(self.render_composer(cx))
            .child(self.commit_panel.clone())
            .into_any_element();
        counters.record_render(render_t0);
        element
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};

    use super::*;
    use gpui::{Focusable, TestAppContext, WindowHandle};
    use tokio_util::sync::CancellationToken;
    use vega_conversation::agent::PermissionHook;
    use vega_conversation::types::{
        Microcents, PermissionDecision, PermissionMode, PermissionRequest, PlanStatus,
        TaskCostSummary, TaskSummaryOutcome, ThreadMode, ThreadStatus, ToolCall, ToolCallStatus,
        ToolResult,
    };
    use vega_markdown::split_deltas;
    use vega_markdown::{ListItem, TableCell};

    // ---------- 锚定状态机（P4） ----------

    use anchor::{AnchorAction as Action, AnchorState as State};

    type DecisionFuture = Pin<Box<dyn Future<Output = PermissionDecision> + Send>>;

    struct StreamHarness {
        stream: Entity<ConversationStream>,
    }

    impl Render for StreamHarness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.stream.clone()
        }
    }

    fn permission_thread() -> Thread {
        Thread {
            id: "thread-safe-id".into(),
            project_id: "project-safe-id".into(),
            title: "Permission test".into(),
            mode: ThreadMode::Execute,
            permission_mode: PermissionMode::Confirm,
            model: "mock".into(),
            status: ThreadStatus::Active,
            pinned: false,
            unread: false,
            created_at: 1,
            updated_at: 1,
        }
    }

    fn init_permission_test(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.set_global(vega_theme::Theme::light());
            cx.set_global(SettingsOpen(false));
            crate::init(cx);
        });
    }

    fn open_permission_stream(
        cx: &mut TestAppContext,
    ) -> (WindowHandle<ConversationStream>, PermissionQueue) {
        let queue = PermissionQueue::new();
        let stream_queue = queue.clone();
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), move |_, cx| {
                cx.new(|cx| {
                    ConversationStream::new_with_permission_queue(
                        permission_thread(),
                        stream_queue,
                        cx,
                    )
                })
            })
            .expect("test window")
        });
        cx.run_until_parked();
        (window, queue)
    }

    fn open_controller_stream(
        cx: &mut TestAppContext,
        thread_id: &str,
    ) -> (
        WindowHandle<StreamHarness>,
        Entity<ConversationStream>,
        Arc<Mutex<Vec<ThreadSettingsRequested>>>,
    ) {
        init_permission_test(cx);
        let mut thread = permission_thread();
        thread.id = thread_id.to_string();
        let stream = cx.new(|cx| ConversationStream::new(thread, cx));
        let root_stream = stream.clone();
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = events.clone();
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), move |_, cx| {
                cx.new(|cx| {
                    cx.subscribe(
                        &root_stream,
                        move |_, _, event: &ThreadSettingsRequested, _| {
                            if let Ok(mut events) = captured.lock() {
                                events.push(event.clone());
                            }
                        },
                    )
                    .detach();
                    StreamHarness {
                        stream: root_stream,
                    }
                })
            })
            .expect("controller stream window")
        });
        cx.run_until_parked();
        (window, stream, events)
    }

    fn focus_setting(
        window: WindowHandle<StreamHarness>,
        stream: &Entity<ConversationStream>,
        index: usize,
        cx: &mut TestAppContext,
    ) {
        window
            .update(cx, |_, window, cx| {
                let focus = stream.read(cx).setting_focus[index].clone();
                window.focus(&focus, cx);
            })
            .expect("settings stream window");
    }

    fn focus_composer(
        window: WindowHandle<StreamHarness>,
        stream: &Entity<ConversationStream>,
        cx: &mut TestAppContext,
    ) {
        window
            .update(cx, |_, window, cx| {
                let focus =
                    stream.read_with(cx, |stream, cx| stream.input.read(cx).focus_handle(cx));
                window.focus(&focus, cx);
            })
            .expect("composer stream window");
    }

    fn bash_call(id: &str, command: &str) -> ToolCall {
        ToolCall {
            id: id.into(),
            tool: "bash".into(),
            input_json: serde_json::json!({ "cmd": command }).to_string(),
        }
    }

    fn propose(window: WindowHandle<ConversationStream>, cx: &mut TestAppContext, call: ToolCall) {
        window
            .update(cx, |stream, _, cx| {
                stream.apply_event(ConversationEvent::ToolCallProposed { call }, cx);
            })
            .expect("stream window");
    }

    fn request_permission(queue: &PermissionQueue, call_id: &str, target: &str) -> DecisionFuture {
        let future = queue.request(
            PermissionRequest {
                call_id: call_id.into(),
                tool: "bash".into(),
                display_target: target.into(),
                danger_rule_id: None,
                danger_reason: None,
            },
            CancellationToken::new(),
        );
        Box::pin(async move { future.await.unwrap_or(PermissionDecision::Timeout) })
    }

    #[gpui::test]
    async fn settings_keyboard_emits_scoped_requests_without_optimistic_state(
        cx: &mut TestAppContext,
    ) {
        let (window, stream, events) = open_controller_stream(cx, "settings-thread");
        focus_setting(window, &stream, 1, cx);
        cx.simulate_keystrokes(window.into(), "enter");
        focus_setting(window, &stream, 5, cx);
        cx.simulate_keystrokes(window.into(), "space");

        let events = events.lock().expect("settings event capture");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].thread_id, "settings-thread");
        assert_eq!(events[0].mode, Some(ThreadMode::Plan));
        assert_eq!(events[0].permission_mode, None);
        assert_eq!(events[1].thread_id, "settings-thread");
        assert_eq!(events[1].mode, None);
        assert_eq!(events[1].permission_mode, Some(PermissionMode::Auto));
        drop(events);

        let selected = stream.read_with(cx, |stream, _| {
            (stream.thread.mode, stream.thread.permission_mode)
        });
        assert_eq!(selected, (ThreadMode::Execute, PermissionMode::Confirm));
        stream.update(cx, ConversationStream::apply_controller_error);
        let selected = stream.read_with(cx, |stream, _| {
            (stream.thread.mode, stream.thread.permission_mode)
        });
        assert_eq!(selected, (ThreadMode::Execute, PermissionMode::Confirm));

        let mut persisted = permission_thread();
        persisted.id = "settings-thread".into();
        persisted.mode = ThreadMode::Plan;
        persisted.permission_mode = PermissionMode::Auto;
        stream.update(cx, |stream, cx| stream.apply_thread(persisted, cx));
        let selected = stream.read_with(cx, |stream, _| {
            (stream.thread.mode, stream.thread.permission_mode)
        });
        assert_eq!(selected, (ThreadMode::Plan, PermissionMode::Auto));
    }

    #[gpui::test]
    async fn multiline_history_continues_and_is_thread_scoped(cx: &mut TestAppContext) {
        let (first_window, first, _) = open_controller_stream(cx, "history-a");
        let (_second_window, second, _) = open_controller_stream(cx, "history-b");
        first.update(cx, |stream, cx| {
            stream.composer_history = vec!["older\nfirst".into(), "newer\nfirst".into()];
            stream
                .input
                .update(cx, |input, cx| input.set_text("draft", cx));
        });
        second.update(cx, |stream, cx| {
            stream.composer_history = vec!["only\nsecond".into()];
            stream
                .input
                .update(cx, |input, cx| input.set_text("second draft", cx));
        });
        focus_composer(first_window, &first, cx);
        cx.simulate_keystrokes(first_window.into(), "up");
        assert_eq!(
            first.read_with(cx, |stream, cx| stream.input.read(cx).text().to_string()),
            "newer\nfirst"
        );
        cx.simulate_keystrokes(first_window.into(), "up");
        assert_eq!(
            first.read_with(cx, |stream, cx| stream.input.read(cx).text().to_string()),
            "older\nfirst"
        );
        assert_eq!(
            second.read_with(cx, |stream, cx| stream.input.read(cx).text().to_string()),
            "second draft"
        );
    }

    #[gpui::test]
    async fn composer_echo_waits_for_durable_acceptance(cx: &mut TestAppContext) {
        let (_window, stream, _) = open_controller_stream(cx, "durable-submit");
        stream.update(cx, |stream, cx| {
            stream
                .input
                .update(cx, |input, cx| input.set_text("keep this draft", cx));
            stream.submit_message(cx);
        });
        let pending = stream.read_with(cx, |stream, cx| {
            (
                stream.composer_submit_pending,
                stream.input.read(cx).text().to_string(),
                stream.composer_history.len(),
                stream.entries.len(),
            )
        });
        assert_eq!(pending, (true, "keep this draft".into(), 0, 0));

        stream.update(cx, ConversationStream::reject_composer_submission);
        let rejected = stream.read_with(cx, |stream, cx| {
            (
                stream.composer_submit_pending,
                stream.input.read(cx).text().to_string(),
                stream.composer_history.len(),
                stream.entries.len(),
            )
        });
        assert_eq!(rejected, (false, "keep this draft".into(), 0, 0));

        stream.update(cx, |stream, cx| {
            stream.submit_message(cx);
            stream.accept_composer_submission("keep this draft", cx);
        });
        let accepted = stream.read_with(cx, |stream, cx| {
            (
                stream.composer_submit_pending,
                stream.input.read(cx).text().to_string(),
                stream.composer_history.clone(),
                stream.entries.len(),
            )
        });
        assert_eq!(
            accepted,
            (false, String::new(), vec!["keep this draft".into()], 1)
        );
    }

    #[gpui::test]
    async fn approved_not_started_projection_preserves_and_blocks_new_draft(
        cx: &mut TestAppContext,
    ) {
        let (_window, stream, _) = open_controller_stream(cx, "approved-recovery");
        stream.update(cx, |stream, cx| {
            stream
                .input
                .update(cx, |input, cx| input.set_text("do not lose", cx));
            stream.apply_approved_not_started(cx);
            stream.submit_message(cx);
        });
        let state = stream.read_with(cx, |stream, cx| {
            (
                stream.approved_not_started,
                stream.composer_submit_pending,
                stream.input.read(cx).text().to_string(),
                stream.entries.len(),
            )
        });
        assert_eq!(state, (true, false, "do not lose".into(), 0));
    }

    #[gpui::test]
    async fn durable_assistant_events_require_exact_active_message(cx: &mut TestAppContext) {
        let (_window, stream, _) = open_controller_stream(cx, "durable-events");
        stream.update(cx, |stream, cx| {
            stream.apply_event(
                ConversationEvent::MessageStarted {
                    message_id: "assistant".into(),
                    seq: 2,
                },
                cx,
            );
            stream.apply_event(
                ConversationEvent::TextDelta {
                    message_id: "foreign".into(),
                    delta: "hidden".into(),
                },
                cx,
            );
        });
        let foreign_ignored = stream.read_with(cx, |stream, _| {
            let (_, index) = stream
                .active_agent_message
                .as_ref()
                .expect("active message");
            match &stream.entries[*index] {
                StreamEntry::Assistant { stream, .. } => stream.snapshot().pending.is_none(),
                _ => false,
            }
        });
        assert!(foreign_ignored);

        stream.update(cx, |stream, cx| {
            stream.apply_event(
                ConversationEvent::TextDelta {
                    message_id: "assistant".into(),
                    delta: "visible".into(),
                },
                cx,
            );
            stream.apply_event(
                ConversationEvent::MessageFinished {
                    message_id: "foreign".into(),
                    stop_reason: vega_conversation::types::ConversationStopReason::End,
                },
                cx,
            );
        });
        assert!(stream.read_with(cx, |stream, _| stream.active_agent_message.is_some()));
        stream.update(cx, |stream, cx| {
            stream.apply_event(
                ConversationEvent::MessageFinished {
                    message_id: "assistant".into(),
                    stop_reason: vega_conversation::types::ConversationStopReason::End,
                },
                cx,
            );
        });
        assert!(stream.read_with(cx, |stream, _| stream.active_agent_message.is_none()));
    }

    #[gpui::test]
    async fn completed_plan_replaces_streaming_assistant_after_older_plan_refresh(
        cx: &mut TestAppContext,
    ) {
        let (_window, stream, _) = open_controller_stream(cx, "plan-dedup");
        stream.update(cx, |stream, cx| {
            stream.apply_event(
                ConversationEvent::MessageStarted {
                    message_id: "plan-message".into(),
                    seq: 2,
                },
                cx,
            );
            stream.apply_event(
                ConversationEvent::TextDelta {
                    message_id: "plan-message".into(),
                    delta: "1. inspect".into(),
                },
                cx,
            );
            stream.apply_event(
                ConversationEvent::MessageFinished {
                    message_id: "plan-message".into(),
                    stop_reason: vega_conversation::types::ConversationStopReason::End,
                },
                cx,
            );
            stream.apply_plan(
                Plan {
                    id: "older-plan".into(),
                    thread_id: "plan-dedup".into(),
                    content: "older".into(),
                    status: PlanStatus::Abandoned,
                    review_note: Some("superseded".into()),
                    reviewed_at: Some(1),
                },
                cx,
            );
            stream.apply_plan(
                Plan {
                    id: "plan-message".into(),
                    thread_id: "plan-dedup".into(),
                    content: "1. inspect".into(),
                    status: PlanStatus::Pending,
                    review_note: None,
                    reviewed_at: None,
                },
                cx,
            );
        });
        let (plans, assistants, entries) = stream.read_with(cx, |stream, _| {
            let plans = stream
                .entries
                .iter()
                .filter(|entry| matches!(entry, StreamEntry::Plan { .. }))
                .count();
            let assistants = stream
                .entries
                .iter()
                .filter(|entry| matches!(entry, StreamEntry::Assistant { .. }))
                .count();
            (plans, assistants, stream.entries.len())
        });
        assert_eq!((plans, assistants, entries), (2, 0, 2));
    }

    #[gpui::test]
    async fn task_summary_card_appends_once_and_ignores_duplicates(cx: &mut TestAppContext) {
        let (_window, stream, _) = open_controller_stream(cx, "summary-card");
        let summary = TaskCostSummary {
            message_id: "assistant-summary".into(),
            outcome: TaskSummaryOutcome::Completed,
            usage: Some(vega_conversation::types::TokenUsage {
                input: 150_000,
                output: 15_000,
                cache_read: 50_000,
                cache_write: 0,
            }),
            cost: vega_conversation::types::SummaryCost::Priced(
                vega_conversation::types::Microcents(135_000),
            ),
            duration_ms: Some(12_400),
            tool_count: 2,
            cache_hit_percent: Some(33),
        };
        stream.update(cx, |stream, cx| {
            stream.apply_task_summary(summary.clone(), cx);
            stream.apply_task_summary(summary, cx);
        });
        let (summaries, rows, text) = stream.read_with(cx, |stream, cx| {
            let mut text = String::new();
            let mut summaries = 0;
            let mut rows = 0;
            for entry in &stream.entries {
                rows += entry.row_count(cx);
                if let StreamEntry::Summary { card } = entry {
                    summaries += 1;
                    text = card.read(cx).visible_text();
                }
            }
            (summaries, rows, text)
        });
        assert_eq!(summaries, 1, "duplicate/stale summaries are ignored");
        assert_eq!(rows, 5, "the card contributes its five fixed rows");
        assert!(text.contains("任务摘要 · 完成"));
        assert!(text.contains("成本 US$0.135000"));
        assert!(text.contains("耗时 12.4s"));
        assert!(text.contains("工具 2 · 缓存命中 33%"));
    }

    fn has_active_permission(
        window: WindowHandle<ConversationStream>,
        cx: &mut TestAppContext,
    ) -> bool {
        window
            .update(cx, |stream, _, _| stream.active_permission.is_some())
            .unwrap_or(false)
    }

    #[gpui::test]
    async fn permission_queue_installs_matching_card_and_once_resolves(cx: &mut TestAppContext) {
        init_permission_test(cx);
        let (window, queue) = open_permission_stream(cx);
        propose(window, cx, bash_call("call-once", "printf ok"));
        let future = request_permission(&queue, "call-once", "printf ok");
        cx.run_until_parked();
        assert!(has_active_permission(window, cx));

        cx.simulate_keystrokes(window.into(), "enter");
        assert_eq!(future.await, PermissionDecision::Once);
        cx.run_until_parked();
        assert!(!has_active_permission(window, cx));
    }

    #[gpui::test]
    async fn permission_target_mismatch_times_out_and_corrupts_tool_card(cx: &mut TestAppContext) {
        init_permission_test(cx);
        let (window, queue) = open_permission_stream(cx);
        propose(window, cx, bash_call("call-mismatch", "printf safe"));
        let future = request_permission(&queue, "call-mismatch", "printf different");
        cx.run_until_parked();
        assert_eq!(future.await, PermissionDecision::Timeout);
        assert!(!has_active_permission(window, cx));
        let visible = window
            .update(cx, |stream, _, cx| {
                stream.tool_cards["call-mismatch"].read(cx).visible_text()
            })
            .expect("stream window");
        assert!(visible.contains("工具结果损坏"));
        assert!(!visible.contains("printf different"));
    }

    #[gpui::test]
    async fn late_permission_requests_for_approved_terminal_or_corrupt_cards_timeout(
        cx: &mut TestAppContext,
    ) {
        init_permission_test(cx);
        let (window, queue) = open_permission_stream(cx);

        propose(window, cx, bash_call("call-approved", "printf approved"));
        window
            .update(cx, |stream, _, cx| {
                stream.apply_event(
                    ConversationEvent::ToolCallApproved {
                        call_id: "call-approved".into(),
                        approval: vega_conversation::types::Approval::Once,
                    },
                    cx,
                );
            })
            .expect("stream window");
        let future = request_permission(&queue, "call-approved", "printf approved");
        cx.run_until_parked();
        assert_eq!(future.await, PermissionDecision::Timeout);
        assert!(!has_active_permission(window, cx));

        propose(
            window,
            cx,
            bash_call("call-terminal-late", "printf terminal"),
        );
        window
            .update(cx, |stream, _, cx| {
                stream.apply_event(
                    ConversationEvent::ToolCallFinished {
                        call_id: "call-terminal-late".into(),
                        result: ToolResult {
                            status: ToolCallStatus::Rejected,
                            output: "Tool error: permission denied".into(),
                            reused: false,
                            exit_code: None,
                            duration_ms: None,
                            truncated: None,
                            invalid: None,
                        },
                    },
                    cx,
                );
            })
            .expect("stream window");
        let future = request_permission(&queue, "call-terminal-late", "printf terminal");
        cx.run_until_parked();
        assert_eq!(future.await, PermissionDecision::Timeout);
        assert!(!has_active_permission(window, cx));

        propose(
            window,
            cx,
            ToolCall {
                id: "call-corrupt".into(),
                tool: "bash".into(),
                input_json: r#"{"cmd":1}"#.into(),
            },
        );
        let future = request_permission(&queue, "call-corrupt", "printf corrupt");
        cx.run_until_parked();
        assert_eq!(future.await, PermissionDecision::Timeout);
        assert!(!has_active_permission(window, cx));
        let permission_entries = window
            .update(cx, |stream, _, _| {
                stream
                    .entries
                    .iter()
                    .filter(|entry| matches!(entry, StreamEntry::Permission { .. }))
                    .count()
            })
            .expect("stream window");
        assert_eq!(permission_entries, 0);
    }

    #[gpui::test]
    async fn settings_hidden_and_terminal_paths_fail_closed_without_rendering(
        cx: &mut TestAppContext,
    ) {
        init_permission_test(cx);
        let (window, queue) = open_permission_stream(cx);
        propose(window, cx, bash_call("call-settings", "printf settings"));
        let future = request_permission(&queue, "call-settings", "printf settings");
        cx.run_until_parked();
        assert!(has_active_permission(window, cx));
        cx.update(|cx| cx.set_global(SettingsOpen(true)));
        cx.run_until_parked();
        assert_eq!(future.await, PermissionDecision::Timeout);
        assert!(!has_active_permission(window, cx));

        cx.update(|cx| cx.set_global(SettingsOpen(false)));
        propose(window, cx, bash_call("call-terminal", "printf terminal"));
        let future = request_permission(&queue, "call-terminal", "printf terminal");
        cx.run_until_parked();
        assert!(has_active_permission(window, cx));
        window
            .update(cx, |stream, _, cx| {
                stream.apply_event(
                    ConversationEvent::ToolCallFinished {
                        call_id: "call-terminal".into(),
                        result: ToolResult {
                            status: ToolCallStatus::Rejected,
                            output: "Tool error: permission denied".into(),
                            reused: false,
                            exit_code: None,
                            duration_ms: None,
                            truncated: None,
                            invalid: None,
                        },
                    },
                    cx,
                );
            })
            .expect("stream window");
        assert_eq!(future.await, PermissionDecision::Timeout);
        assert!(!has_active_permission(window, cx));

        cx.update(|cx| cx.set_global(SettingsOpen(true)));
        propose(window, cx, bash_call("call-hidden", "printf hidden"));
        let future = request_permission(&queue, "call-hidden", "printf hidden");
        cx.run_until_parked();
        assert_eq!(future.await, PermissionDecision::Timeout);
        assert!(!has_active_permission(window, cx));
    }

    #[gpui::test]
    async fn window_release_drops_listener_and_active_card_fail_closed(cx: &mut TestAppContext) {
        init_permission_test(cx);
        let (window, queue) = open_permission_stream(cx);
        propose(window, cx, bash_call("call-window", "printf close"));
        let future = request_permission(&queue, "call-window", "printf close");
        cx.run_until_parked();
        assert!(has_active_permission(window, cx));
        window
            .update(cx, |_, window, _| window.remove_window())
            .expect("stream window");
        cx.run_until_parked();
        assert_eq!(future.await, PermissionDecision::Timeout);
    }

    #[gpui::test]
    async fn thread_switch_timeout_contract_removes_prompt_before_view_replacement(
        cx: &mut TestAppContext,
    ) {
        init_permission_test(cx);
        let (window, queue) = open_permission_stream(cx);
        propose(window, cx, bash_call("call-thread", "printf switch"));
        let future = request_permission(&queue, "call-thread", "printf switch");
        cx.run_until_parked();
        assert!(has_active_permission(window, cx));
        window
            .update(cx, |stream, _, cx| stream.timeout_permission(cx))
            .expect("stream window");
        assert_eq!(future.await, PermissionDecision::Timeout);
        assert!(!has_active_permission(window, cx));
    }

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
        let lines = flatten_nodes(7, &[node], BlockOrigin::Committed);
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
        let lines = flatten_nodes(9, &[node], BlockOrigin::Committed);
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
        let lines = flatten_nodes(11, &[node], BlockOrigin::Committed);
        // 尾换行不产生空行。
        assert_eq!(lines.len(), 3);
        assert!(lines.iter().all(|line| line.kind == LineKind::Code));
        assert_eq!(spans_text(&lines[1]), "    let x = 1;");
    }

    // ---------- T18 高亮整合（committed 高亮 / pending 降级） ----------

    fn find_span<'a>(lines: &'a [StreamLine], text: &str) -> &'a StreamSpan {
        lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .find(|span| span.text == text)
            .unwrap_or_else(|| panic!("span {text:?} not found"))
    }

    #[test]
    fn committed_code_block_carries_highlight_token_kinds() {
        let node = RenderNode::CodeBlock {
            language: Some("rust".into()),
            code: "fn main() {\n    let n = 42;\n}\n".into(),
        };
        let lines = flatten_nodes(21, &[node], BlockOrigin::Committed);
        // 关键字 → Token(Keyword)；函数名 → Token(Function)（映射表「其余」
        // 档）；rust grammar 把整数字面量捕获为 constant.builtin →
        // Token(Constant)；行内未被捕获的文字补 Plain。
        assert_eq!(
            find_span(&lines, "fn").style,
            SpanStyle::Token(HighlightKind::Keyword)
        );
        assert_eq!(
            find_span(&lines, "main").style,
            SpanStyle::Token(HighlightKind::Function)
        );
        assert_eq!(
            find_span(&lines, "let").style,
            SpanStyle::Token(HighlightKind::Keyword)
        );
        assert_eq!(
            find_span(&lines, "42").style,
            SpanStyle::Token(HighlightKind::Constant)
        );
        assert_eq!(find_span(&lines, "    ").style, SpanStyle::Plain);
    }

    #[test]
    fn pending_tail_and_unsupported_language_stay_plain_monospace() {
        let node = RenderNode::CodeBlock {
            language: Some("rust".into()),
            code: "fn a() {}\n".into(),
        };
        // 未闭合 fence（pending 尾块）降级纯文本（tech-spec §5.1）。
        let lines = flatten_nodes(23, &[node], BlockOrigin::Pending);
        assert!(
            lines
                .iter()
                .all(|line| line.spans.iter().all(|span| span.style == SpanStyle::Plain))
        );
        // 未支持语言同样降级。
        let unknown = RenderNode::CodeBlock {
            language: Some("cobol".into()),
            code: "MOVE 1 TO X.\n".into(),
        };
        let lines = flatten_nodes(24, &[unknown], BlockOrigin::Committed);
        assert!(
            lines
                .iter()
                .all(|line| line.spans.iter().all(|span| span.style == SpanStyle::Plain))
        );
    }

    #[test]
    fn code_line_spans_fill_gaps_and_clip_at_line_edges() {
        // CJK 与多行切割：高亮 span 按字节切片，缺口补 Plain，逐行覆盖完整。
        let code = "let s = \"中文\";\nlet t = 1;\n";
        let node = RenderNode::CodeBlock {
            language: Some("rust".into()),
            code: code.to_string(),
        };
        let lines = flatten_nodes(25, &[node], BlockOrigin::Committed);
        assert_eq!(lines.len(), 2);
        assert_eq!(spans_text(&lines[0]), "let s = \"中文\";");
        assert_eq!(spans_text(&lines[1]), "let t = 1;");
        // 字符串（含 CJK 字面量）应整体有 String 捕获（转义无关），按行切片
        // 后行内仍存在 String span。
        assert!(
            lines[0]
                .spans
                .iter()
                .any(|span| span.style == SpanStyle::Token(HighlightKind::String))
        );
    }

    // ---------- T18 消息块（user 回显行模型） ----------

    #[test]
    fn user_message_lines_materialize_label_card_and_spacer() {
        let lines = user_message_lines(USER_BLOCK_BASE, "第一行\n\n第三行");
        assert_eq!(lines.len(), 5);
        assert_eq!(lines[0].kind, LineKind::UserLabel);
        assert_eq!(
            lines[1].kind,
            LineKind::UserLine {
                first: true,
                last: false
            }
        );
        // 中间空行也是卡片行（连续背景）。
        assert_eq!(
            lines[2].kind,
            LineKind::UserLine {
                first: false,
                last: false
            }
        );
        assert_eq!(
            lines[3].kind,
            LineKind::UserLine {
                first: false,
                last: true
            }
        );
        assert_eq!(lines[4].kind, LineKind::Spacer);
        assert_eq!(spans_text(&lines[1]), "第一行");
        assert_eq!(spans_text(&lines[2]), "");
        // 尾换行不产生尾部空卡片行。
        assert_eq!(user_message_lines(1, "hi\n").len(), 3);
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
        let lines = flatten_nodes(13, &[node], BlockOrigin::Committed);
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
        let lines = flatten_nodes(15, &[node], BlockOrigin::Committed);
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

    // ---------- Composer token counter（S7-T39/A10-05） ----------

    #[gpui::test]
    async fn composer_counter_projects_estimate_calibration_and_fences(cx: &mut TestAppContext) {
        let (_window, stream, _) = open_controller_stream(cx, "meter-thread");
        // Unpriced start: the counter is visible (not noise) and shows `—`.
        let initial = stream.read_with(cx, |stream, _| stream.meter_snapshot());
        assert_eq!(initial.display(), "0 tok · —");

        stream.update(cx, |stream, cx| {
            stream.install_meter_estimator(
                // `RunUsageEstimator::new` is already `Option`: an unpriced
                // model yields `None` and the counter shows `—`.
                RunUsageEstimator::new(
                    "meter-model",
                    vega_conversation::PricingCatalog::from_specs(vec![
                        vega_conversation::ModelPricingSpec {
                            model: "meter-model".into(),
                            rates: vega_conversation::RateSpec {
                                input_usd_per_million: "1".into(),
                                output_usd_per_million: "2".into(),
                                cache_read_usd_per_million: "0.1".into(),
                                cache_write_usd_per_million: "0".into(),
                            },
                            max_standard_input_tokens: None,
                            schedule: None,
                        },
                    ])
                    .expect("catalog"),
                ),
                cx,
            );
            stream.apply_event(
                ConversationEvent::MessageStarted {
                    message_id: "assistant".into(),
                    seq: 1,
                },
                cx,
            );
            stream.apply_event(
                ConversationEvent::TextDelta {
                    message_id: "assistant".into(),
                    delta: "中文🦀".into(),
                },
                cx,
            );
        });
        let streaming = stream.read_with(cx, |stream, _| stream.meter_snapshot());
        assert_eq!(streaming.tokens, 1, "3 unicode scalars ceil-divided by 4");
        assert!(streaming.provisional);
        assert_eq!(streaming.display(), "≈1 tok · ≈US$0.000002");

        // Calibration replaces the estimate in place; late duplicate usage on
        // the finished message cannot re-add.
        stream.update(cx, |stream, cx| {
            stream.apply_event(
                ConversationEvent::UsageUpdated {
                    message_id: "assistant".into(),
                    usage: vega_conversation::types::TokenUsage {
                        input: 100,
                        output: 10,
                        cache_read: 0,
                        cache_write: 0,
                    },
                    cost: Microcents(120),
                    pricing: Some(vega_conversation::types::UsagePricing {
                        version: "pricing_v1".into(),
                        profile: "base".into(),
                        call_started_at: 1_700_000_000,
                    }),
                },
                cx,
            );
            stream.apply_event(
                ConversationEvent::MessageFinished {
                    message_id: "assistant".into(),
                    stop_reason: vega_conversation::types::ConversationStopReason::End,
                },
                cx,
            );
        });
        let calibrated = stream.read_with(cx, |stream, _| stream.meter_snapshot());
        assert_eq!(calibrated.display(), "110 tok · US$0.00012");

        // Route fence: a late text delta for the finished message must not
        // resurrect the provisional counter.
        stream.update(cx, |stream, cx| {
            stream.apply_event(
                ConversationEvent::TextDelta {
                    message_id: "assistant".into(),
                    delta: "late arrival".into(),
                },
                cx,
            );
        });
        let fenced = stream.read_with(cx, |stream, _| stream.meter_snapshot());
        assert_eq!(fenced.display(), "110 tok · US$0.00012");

        // Restart recovery: the restored aggregate becomes the new baseline.
        stream.update(cx, |stream, cx| {
            stream.restore_meter(
                RestoredUsage {
                    tokens: 1_234_567,
                    cost: Some(Microcents(180_000)),
                },
                cx,
            );
        });
        let restored = stream.read_with(cx, |stream, _| stream.meter_snapshot());
        assert_eq!(restored.display(), "1.2M tok · US$0.18");
    }

    #[gpui::test]
    async fn composer_counter_error_path_clears_provisional(cx: &mut TestAppContext) {
        let (_window, stream, _) = open_controller_stream(cx, "meter-error-thread");
        stream.update(cx, |stream, cx| {
            stream.apply_event(
                ConversationEvent::MessageStarted {
                    message_id: "assistant".into(),
                    seq: 1,
                },
                cx,
            );
            stream.apply_event(
                ConversationEvent::TextDelta {
                    message_id: "assistant".into(),
                    delta: "abcd".into(),
                },
                cx,
            );
        });
        assert!(stream.read_with(cx, |stream, _| stream.meter_snapshot().provisional));
        // Controller failure (spawn error etc.) clears run-scoped state.
        stream.update(cx, ConversationStream::apply_agent_error);
        let cleared = stream.read_with(cx, |stream, _| stream.meter_snapshot());
        assert!(!cleared.provisional);
        assert_eq!(cleared.display(), "0 tok · —");
    }
}
