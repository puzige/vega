//! [`MarkdownStream`]: mdstream 0.3.0 incremental chunking + a per-`BlockId`
//! frozen render-node cache + borrowed snapshots for UI diffing.
//!
//! Layering (tech-spec §5.1, route A):
//!
//! ```text
//! append(delta) ─▶ mdstream::MdStream ─▶ Update { committed, pending, reset, invalidated }
//!                    │                        │
//!                    │                        ├─ committed ─▶ PulldownAdapter（按 BlockId
//!                    │                        │   冻结事件缓存，引用定义 prelude 注入）
//!                    │                        │      └─▶ RenderNode 冻结缓存（仅首次转换）
//!                    │                        └─ pending（terminator 补全 display）─▶ 轻量重解析
//! snapshot() ─▶ 有序 BlockView + pending 尾块，UI 按 (block_id, version) 做差量
//! ```
//!
//! The stream is a single-owner synchronous data structure (no tokio, no
//! locks): delta coalescing / throttling is the caller's job (tech-spec §5.1;
//! spike 实测 1.23 µs/delta，1k delta/s 占单核 0.12%，攒批必要性低).

use std::collections::HashMap;

use mdstream::adapters::pulldown::{PulldownAdapter, PulldownAdapterOptions};
use mdstream::{MdStream, Options, ReferenceDefinitionsMode, Update};
use pulldown_cmark::Options as PulldownOptions;

#[cfg(test)]
use crate::nodes::Inline;
use crate::nodes::{RenderNode, render_nodes_from_events};

/// GFM extensions enabled for block parsing (tech-spec §5.0：tables /
/// tasklists / strikethrough 全开).
const PULLDOWN_GFM: PulldownOptions = PulldownOptions::ENABLE_TABLES
    .union(PulldownOptions::ENABLE_STRIKETHROUGH)
    .union(PulldownOptions::ENABLE_TASKLISTS);

/// One frozen committed block: parsed exactly once per version.
struct CommittedEntry {
    block_id: u64,
    version: u64,
    nodes: Vec<RenderNode>,
}

/// The single pending (still-streaming) tail block, lightly re-parsed per
/// append and never stored in the frozen cache.
struct PendingEntry {
    block_id: u64,
    version: u64,
    nodes: Vec<RenderNode>,
}

/// Borrowed view of one committed block.
#[derive(Debug, Clone, Copy)]
pub struct BlockView<'a> {
    /// mdstream `BlockId`（committed 块一旦产出永不变更，UI 可安全缓存）.
    pub block_id: u64,
    /// Parse version: 1 on first freeze, +1 per invalidation re-parse.
    pub version: u64,
    /// Frozen render nodes for this block.
    pub nodes: &'a [RenderNode],
}

/// Borrowed view of the pending tail block (`None` when nothing is streaming).
#[derive(Debug, Clone, Copy)]
pub struct PendingView<'a> {
    pub block_id: u64,
    /// Bumped on every light re-parse (once per append carrying a tail), so
    /// the UI can skip unchanged ticks if it ever needs to.
    pub version: u64,
    /// Nodes parsed from mdstream's terminator-completed display view.
    pub nodes: &'a [RenderNode],
}

/// Ordered snapshot of the whole stream state.
///
/// Borrows from the stream (zero-copy): the UI compares `(block_id, version)`
/// against its previous frame and only materializes changed blocks.
#[derive(Debug, Clone)]
pub struct StreamSnapshot<'a> {
    /// Committed blocks in document order.
    pub blocks: Vec<BlockView<'a>>,
    /// The pending tail block, if any.
    pub pending: Option<PendingView<'a>>,
}

/// Streaming Markdown pipeline: append deltas, snapshot render instructions.
pub struct MarkdownStream {
    stream: MdStream,
    adapter: PulldownAdapter,
    /// Frozen committed entries in document order.
    committed: Vec<CommittedEntry>,
    /// block_id → position in `committed`.
    index: HashMap<u64, usize>,
    pending: Option<PendingEntry>,
    #[cfg(test)]
    parse_counts: HashMap<u64, u32>,
    #[cfg(test)]
    pending_parse_ticks: u32,
}

impl Default for MarkdownStream {
    fn default() -> Self {
        Self::new()
    }
}

impl MarkdownStream {
    /// Creates a stream with GFM tables/tasklists/strikethrough enabled and
    /// mdstream's `Invalidate` reference-definition mode: late `[ref]: url`
    /// definitions re-parse exactly the blocks that used the label
    /// (tech-spec §5.1).
    pub fn new() -> Self {
        let options = Options {
            reference_definitions: ReferenceDefinitionsMode::Invalidate,
            ..Options::default()
        };
        let adapter_options = PulldownAdapterOptions {
            pulldown: PULLDOWN_GFM,
            prefer_display_for_pending: true,
        };
        Self {
            stream: MdStream::new(options),
            adapter: PulldownAdapter::new(adapter_options),
            committed: Vec::new(),
            index: HashMap::new(),
            pending: None,
            #[cfg(test)]
            parse_counts: HashMap::new(),
            #[cfg(test)]
            pending_parse_ticks: 0,
        }
    }

    /// Appends one streamed delta and updates the incremental caches.
    pub fn append(&mut self, delta: &str) {
        let update = self.stream.append(delta);
        self.absorb(update);
    }

    /// Finalizes the stream (tech-spec §5.4 final 终结语义): the pending
    /// terminator-completed view is discarded and the tail block is re-parsed
    /// once from its final raw content, then frozen as a committed block.
    ///
    /// Idempotent: finishing twice is a no-op the second time.
    pub fn finish(&mut self) {
        let update = self.stream.finalize();
        self.absorb(update);
    }

    /// Borrowed ordered snapshot of committed blocks plus the pending tail.
    pub fn snapshot(&self) -> StreamSnapshot<'_> {
        StreamSnapshot {
            blocks: self
                .committed
                .iter()
                .map(|entry| BlockView {
                    block_id: entry.block_id,
                    version: entry.version,
                    nodes: &entry.nodes,
                })
                .collect(),
            pending: self.pending.as_ref().map(|entry| PendingView {
                block_id: entry.block_id,
                version: entry.version,
                nodes: &entry.nodes,
            }),
        }
    }

    /// Folds one mdstream `Update` into the frozen cache and pending view.
    fn absorb(&mut self, update: Update) {
        if update.reset {
            // tech-spec §5.0：reset（footnote 单块切换等 scope 转换，罕见）
            // = 丢弃全部缓存按本条重建；PulldownAdapter 在 apply_update 内自清
            self.committed.clear();
            self.index.clear();
            self.pending = None;
            #[cfg(test)]
            {
                self.parse_counts.clear();
                self.pending_parse_ticks = 0;
            }
        }
        self.adapter.apply_update(&update);

        // committed：BlockId 稳定、内容不变 —— 只在首次出现时转换并冻结
        for block in &update.committed {
            if self.index.contains_key(&block.id.0) {
                // 冻结承诺：重复上报的 committed 块绝不重解析（防御性跳过）
                continue;
            }
            let Some(events) = self.adapter.committed_events(block.id) else {
                // apply_update 刚写入该块，正常必中；缺省跳过而非 panic
                continue;
            };
            let nodes = render_nodes_from_events(events);
            self.index.insert(block.id.0, self.committed.len());
            self.committed.push(CommittedEntry {
                block_id: block.id.0,
                version: 1,
                nodes,
            });
            #[cfg(test)]
            {
                *self.parse_counts.entry(block.id.0).or_insert(0) += 1;
            }
        }

        // invalidated：后置引用定义等文档级语义波及 —— 按 BlockId 重解析指定块
        for id in &update.invalidated {
            let Some(&position) = self.index.get(&id.0) else {
                continue;
            };
            let Some(events) = self.adapter.committed_events(*id) else {
                continue;
            };
            let nodes = render_nodes_from_events(events);
            let entry = &mut self.committed[position];
            entry.nodes = nodes;
            entry.version += 1;
            #[cfg(test)]
            {
                *self.parse_counts.entry(id.0).or_insert(0) += 1;
            }
        }

        // pending 尾块：每次 append 后按 terminator 补全视图轻量重解析，不入冻结缓存
        match update.pending {
            Some(pending) => {
                let events = self.adapter.parse_pending(&pending);
                let nodes = render_nodes_from_events(&events);
                let version = match self.pending.as_ref() {
                    Some(previous) if previous.block_id == pending.id.0 => previous.version + 1,
                    _ => 1,
                };
                self.pending = Some(PendingEntry {
                    block_id: pending.id.0,
                    version,
                    nodes,
                });
                #[cfg(test)]
                {
                    self.pending_parse_ticks += 1;
                }
            }
            None => self.pending = None,
        }
    }
}

#[cfg(test)]
impl MarkdownStream {
    /// Times the block with `block_id` has been converted into render nodes.
    ///
    /// This is the observable proxy for "解析次数": the PulldownAdapter parses
    /// each committed block exactly once per commit (plus invalidation), and
    /// the frozen-cache contract is that the conversion side matches it —
    /// exactly 1 for every committed block in a plain stream.
    fn parse_count(&self, block_id: u64) -> u32 {
        self.parse_counts.get(&block_id).copied().unwrap_or(0)
    }

    fn total_parse_count(&self) -> u32 {
        self.parse_counts.values().sum()
    }

    fn pending_tick_count(&self) -> u32 {
        self.pending_parse_ticks
    }

    fn committed_ids(&self) -> Vec<u64> {
        self.committed.iter().map(|entry| entry.block_id).collect()
    }

    /// Coarse frozen-cache footprint in text bytes (render nodes only).
    fn cache_bytes(&self) -> usize {
        self.committed
            .iter()
            .map(|entry| node_bytes(&entry.nodes))
            .sum::<usize>()
            + self
                .pending
                .as_ref()
                .map_or(0, |entry| node_bytes(&entry.nodes))
    }
}

/// Rough text footprint of a render-node forest (test-only metric).
#[cfg(test)]
fn node_bytes(nodes: &[RenderNode]) -> usize {
    fn inline_bytes(spans: &[Inline]) -> usize {
        spans
            .iter()
            .map(|span| match span {
                Inline::Text(text) | Inline::Code(text) => text.len(),
                Inline::Emphasis(inner) | Inline::Strong(inner) | Inline::Strikethrough(inner) => {
                    inline_bytes(inner)
                }
                Inline::Link { title, spans, .. } => {
                    inline_bytes(spans) + title.as_deref().map_or(0, str::len)
                }
            })
            .sum()
    }
    fn block_bytes(nodes: &[RenderNode]) -> usize {
        nodes
            .iter()
            .map(|node| match node {
                RenderNode::Paragraph { spans } => inline_bytes(spans),
                RenderNode::Heading { spans, .. } => inline_bytes(spans),
                RenderNode::CodeBlock { code, .. } => code.len(),
                RenderNode::List(list) => list
                    .items
                    .iter()
                    .map(|item| block_bytes(&item.children))
                    .sum(),
                RenderNode::BlockQuote { children } => block_bytes(children),
                RenderNode::Table(table) => {
                    table
                        .header
                        .iter()
                        .map(|c| inline_bytes(&c.spans))
                        .sum::<usize>()
                        + table
                            .rows
                            .iter()
                            .map(|row| row.iter().map(|c| inline_bytes(&c.spans)).sum::<usize>())
                            .sum::<usize>()
                }
                RenderNode::ThematicBreak => 0,
            })
            .sum()
    }
    block_bytes(nodes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Inline, ListBlock, ListItem, TableAlignment, TableBlock, TableCell};

    // ---------- test helpers ----------

    /// Splits `doc` into 3..8 byte token-like deltas (never splitting a UTF-8
    /// codepoint), mirroring the T14 spike harness.
    fn split_deltas(doc: &str, seed: u64) -> Vec<&str> {
        let bytes = doc.as_bytes();
        let mut deltas = Vec::new();
        let mut pos = 0;
        let mut state = seed;
        while pos < bytes.len() {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let step = 3 + (state >> 33) as usize % 6;
            let mut end = (pos + step).min(bytes.len());
            while end < bytes.len() && (bytes[end] & 0xC0) == 0x80 {
                end += 1;
            }
            deltas.push(std::str::from_utf8(&bytes[pos..end]).unwrap());
            pos = end;
        }
        deltas
    }

    fn stream_all(doc: &str) -> MarkdownStream {
        let mut stream = MarkdownStream::new();
        for delta in split_deltas(doc, 0x5EED) {
            stream.append(delta);
        }
        stream
    }

    fn text(s: &str) -> Inline {
        Inline::Text(s.to_string())
    }

    fn para(spans: Vec<Inline>) -> RenderNode {
        RenderNode::Paragraph { spans }
    }

    fn code(language: &str, code: &str) -> RenderNode {
        RenderNode::CodeBlock {
            language: Some(language.to_string()),
            code: code.to_string(),
        }
    }

    /// Synthetic mixed-CJK streaming document, probe-style (no footnotes, so
    /// mdstream's default single-block footnote mode stays dormant).
    fn long_doc(lines: usize) -> String {
        let cjk = [
            "这是一段中文文本，用于验证混排流式解析。",
            "中文与 English 混排的场景非常常见，需要稳定。",
            "表格行内包含中日韩字符时宽度测量更要小心。",
        ];
        let mut doc = String::with_capacity(64 * 1024);
        let mut i = 0;
        while doc.lines().count() < lines {
            let zh = cjk[i % cjk.len()];
            match i % 8 {
                0 => doc.push_str(&format!("## Section {i}: streaming markdown\n\n")),
                1 => doc.push_str(&format!(
                    "Paragraph {i} with **bold**, *italic*, `inline code`, \
                     [link](https://example.com/{i}) and ~~strike~~. {zh}\n\n"
                )),
                2 => doc.push_str(&format!(
                    "| A {i} | B | C |\n|---|---|---|\n| 1 | {zh} | 3 |\n| 4 | 5 | 6 |\n\n"
                )),
                3 => doc.push_str(&format!(
                    "- task one {i}\n- [ ] pending\n- [x] done\n- nested `code`\n\n"
                )),
                4 => doc.push_str(&format!(
                    "```rust\nfn example_{i}() -> u64 {{\n    let v = {i} * 42;\n    v\n}}\n```\n\n"
                )),
                5 => doc.push_str(&format!("> quote line 1 {i}\n> quote line 2 {zh}\n\n")),
                6 => doc.push_str(&format!(
                    "1. ordered alpha {i}\n2. ordered beta\n   - sub item\n\n"
                )),
                _ => doc.push_str(&format!("Plain tail paragraph {i}. {zh}\n\n")),
            }
            i += 1;
        }
        doc
    }

    // ---------- delta 流 → snapshot 断言 ----------

    #[test]
    fn paragraphs_and_headings_stream_into_ordered_snapshot() {
        let doc = "# Title\n\nIntro paragraph.\n\n## Sub\n";
        let mut stream = MarkdownStream::new();
        for delta in split_deltas(doc, 7) {
            stream.append(delta);
        }
        stream.finish();
        let snapshot = stream.snapshot();
        assert!(snapshot.pending.is_none());
        assert_eq!(snapshot.blocks.len(), 3);
        assert_eq!(snapshot.blocks[0].version, 1);
        assert_eq!(
            snapshot.blocks[0].nodes,
            &[RenderNode::Heading {
                level: 1,
                spans: vec![text("Title")]
            }][..]
        );
        assert_eq!(
            snapshot.blocks[1].nodes,
            &[para(vec![text("Intro paragraph.")])][..]
        );
        assert_eq!(
            snapshot.blocks[2].nodes,
            &[RenderNode::Heading {
                level: 2,
                spans: vec![text("Sub")]
            }][..]
        );
    }

    #[test]
    fn unordered_and_ordered_lists_nest() {
        let doc = "- alpha\n- beta\n  - inner\n\n1. first\n2. second\n   1. sub\n";
        let mut stream = stream_all(doc);
        stream.finish();
        let snapshot = stream.snapshot();
        // mdstream 把空行分隔的紧邻两个列表并入一个块；转换层输出两个 List 节点
        assert_eq!(snapshot.blocks.len(), 1);
        assert_eq!(
            snapshot.blocks[0].nodes,
            &[
                RenderNode::List(ListBlock {
                    ordered: false,
                    start: 1,
                    items: vec![
                        ListItem {
                            checked: None,
                            children: vec![para(vec![text("alpha")])],
                        },
                        ListItem {
                            checked: None,
                            children: vec![
                                para(vec![text("beta")]),
                                RenderNode::List(ListBlock {
                                    ordered: false,
                                    start: 1,
                                    items: vec![ListItem {
                                        checked: None,
                                        children: vec![para(vec![text("inner")])],
                                    }],
                                }),
                            ],
                        },
                    ],
                }),
                RenderNode::List(ListBlock {
                    ordered: true,
                    start: 1,
                    items: vec![
                        ListItem {
                            checked: None,
                            children: vec![para(vec![text("first")])],
                        },
                        ListItem {
                            checked: None,
                            children: vec![
                                para(vec![text("second")]),
                                RenderNode::List(ListBlock {
                                    ordered: true,
                                    start: 1,
                                    items: vec![ListItem {
                                        checked: None,
                                        children: vec![para(vec![text("sub")])],
                                    }],
                                }),
                            ],
                        },
                    ],
                }),
            ][..]
        );
    }

    #[test]
    fn table_streaming_across_chunk_boundary_yields_one_frozen_table() {
        // 0.3.0 修复的边界：表格分隔行被 delta 从中间切开
        let mut stream = MarkdownStream::new();
        stream.append("| A | B |\n");
        stream.append("|--");
        stream.append("-|--");
        stream.append("-|\n");
        stream.append("| 1 | 2 |\n");
        stream.append("\n");
        stream.finish();
        let snapshot = stream.snapshot();
        assert_eq!(snapshot.blocks.len(), 1);
        assert_eq!(
            snapshot.blocks[0].nodes,
            &[RenderNode::Table(TableBlock {
                alignments: vec![TableAlignment::None, TableAlignment::None],
                header: vec![
                    TableCell {
                        spans: vec![text("A")]
                    },
                    TableCell {
                        spans: vec![text("B")]
                    },
                ],
                rows: vec![vec![
                    TableCell {
                        spans: vec![text("1")]
                    },
                    TableCell {
                        spans: vec![text("2")]
                    },
                ]],
            })][..]
        );
        assert_eq!(stream.parse_count(snapshot.blocks[0].block_id), 1);
    }

    #[test]
    fn inline_styles_mixed_stream() {
        let doc = "plain **bold** *em* ~~del~~ `code` [label](https://example.com) tail\n";
        let mut stream = MarkdownStream::new();
        for delta in split_deltas(doc, 21) {
            stream.append(delta);
        }
        stream.finish();
        let snapshot = stream.snapshot();
        assert_eq!(snapshot.blocks.len(), 1);
        assert_eq!(
            snapshot.blocks[0].nodes,
            &[para(vec![
                text("plain "),
                Inline::Strong(vec![text("bold")]),
                text(" "),
                Inline::Emphasis(vec![text("em")]),
                text(" "),
                Inline::Strikethrough(vec![text("del")]),
                text(" "),
                Inline::Code("code".to_string()),
                text(" "),
                Inline::Link {
                    url: "https://example.com".to_string(),
                    title: None,
                    spans: vec![text("label")],
                },
                text(" tail"),
            ])][..]
        );
    }

    #[test]
    fn code_block_freezes_with_language() {
        let doc = "```rust\nfn main() {}\n```\n";
        let mut stream = stream_all(doc);
        stream.finish();
        let snapshot = stream.snapshot();
        assert_eq!(snapshot.blocks.len(), 1);
        assert_eq!(
            snapshot.blocks[0].nodes,
            &[code("rust", "fn main() {}\n")][..]
        );
    }

    #[test]
    fn unclosed_fence_is_pending_then_upgrades_to_committed() {
        let mut stream = MarkdownStream::new();
        for delta in split_deltas("```rust\nfn main() {\n    println!(\"hi\");\n", 3) {
            stream.append(delta);
        }
        // 流式中途：未闭合 fence 在 pending 尾块，按 terminator 补全视图解析为代码块
        let pending_block_id = {
            let mid = stream.snapshot();
            assert!(mid.blocks.is_empty());
            let pending = mid
                .pending
                .expect("unclosed fence must be the pending tail");
            assert_eq!(
                pending.nodes,
                &[code("rust", "fn main() {\n    println!(\"hi\");\n")][..]
            );
            pending.block_id
        };
        // 闭合后：同一 BlockId 冻结为 committed 代码块（内容补上闭合前的 `}`）
        for delta in split_deltas("}\n```\n\nDone.\n", 5) {
            stream.append(delta);
        }
        stream.finish();
        let snapshot = stream.snapshot();
        assert!(snapshot.pending.is_none());
        assert_eq!(snapshot.blocks.len(), 2);
        assert_eq!(snapshot.blocks[0].block_id, pending_block_id);
        assert_eq!(
            snapshot.blocks[0].nodes,
            &[code("rust", "fn main() {\n    println!(\"hi\");\n}\n")][..]
        );
        assert_eq!(snapshot.blocks[1].nodes, &[para(vec![text("Done.")])][..]);
        assert_eq!(stream.parse_count(snapshot.blocks[0].block_id), 1);
    }

    // ---------- 冻结缓存行为 ----------

    #[test]
    fn committed_blocks_parse_exactly_once_through_a_long_stream() {
        let doc = long_doc(2_000);
        let deltas = split_deltas(&doc, 0x5EED);
        let mut stream = MarkdownStream::new();
        for delta in &deltas {
            stream.append(delta);
        }
        stream.finish();
        let snapshot = stream.snapshot();
        assert!(snapshot.pending.is_none());
        assert!(
            snapshot.blocks.len() > 100,
            "expected many committed blocks"
        );
        // committed 块解析计数恒 1：pending 尾部成千上万次更新未触发任何 committed 重解析
        for block in &snapshot.blocks {
            assert_eq!(
                stream.parse_count(block.block_id),
                1,
                "committed block {} was re-parsed",
                block.block_id
            );
        }
        assert_eq!(stream.total_parse_count(), snapshot.blocks.len() as u32);
        assert!(stream.pending_tick_count() > 1_000);
        assert_eq!(
            stream.committed_ids(),
            snapshot
                .blocks
                .iter()
                .map(|b| b.block_id)
                .collect::<Vec<_>>()
        );
    }

    // ---------- finish 终结语义 ----------

    #[test]
    fn finish_overrides_pending_completion_guess() {
        let mut stream = MarkdownStream::new();
        for delta in split_deltas("stub **bo", 11) {
            stream.append(delta);
        }
        // 流式补全态：terminator 把 **bo 补成 **bo**，pending 视图为 bold
        let pending_block_id = {
            let mid = stream.snapshot();
            let pending = mid.pending.expect("pending tail expected");
            assert_eq!(
                pending.nodes,
                &[para(vec![text("stub "), Inline::Strong(vec![text("bo")])])][..]
            );
            pending.block_id
        };
        // final：作废补全态，以最终语义对原始内容完整重解析一次并冻结
        stream.finish();
        let snapshot = stream.snapshot();
        assert!(snapshot.pending.is_none());
        assert_eq!(snapshot.blocks.len(), 1);
        assert_eq!(snapshot.blocks[0].version, 1);
        assert_eq!(
            snapshot.blocks[0].nodes,
            &[para(vec![text("stub **bo")])][..]
        );
        assert_eq!(stream.parse_count(snapshot.blocks[0].block_id), 1);
        assert_eq!(snapshot.blocks[0].block_id, pending_block_id);
        // 幂等：二次 finish 不产生新解析
        stream.finish();
        assert_eq!(stream.total_parse_count(), 1);
    }

    // ---------- Update.reset / Update.invalidated ----------

    #[test]
    fn late_reference_definition_invalidates_affected_block_only() {
        let doc = "see [doc][ref] now\n\n[ref]: https://example.com/a\n";
        let mut stream = MarkdownStream::new();
        for delta in split_deltas(doc, 9) {
            stream.append(delta);
        }
        stream.finish();
        let snapshot = stream.snapshot();
        assert_eq!(snapshot.blocks.len(), 2);
        // 引用定义后到：第一块按 Update.invalidated 重解析，版本 2；定义块保持版本 1
        assert_eq!(snapshot.blocks[0].version, 2);
        assert_eq!(snapshot.blocks[1].version, 1);
        assert_eq!(
            snapshot.blocks[0].nodes,
            &[para(vec![
                text("see "),
                Inline::Link {
                    url: "https://example.com/a".to_string(),
                    title: None,
                    spans: vec![text("doc")],
                },
                text(" now"),
            ])][..]
        );
        assert_eq!(stream.parse_count(snapshot.blocks[0].block_id), 2);
        assert_eq!(stream.parse_count(snapshot.blocks[1].block_id), 1);
    }

    #[test]
    fn footnote_scope_reset_rebuilds_the_cache() {
        // FootnotesMode::SingleBlock（默认）：检测到脚注引用即触发 Update.reset，
        // 之后整篇文档作为单个 pending 块，finish 时整体冻结为 BlockId(1)。
        // 用整行 append（脚注标记不能被 delta 切开，检测按 chunk 扫描）。
        let mut stream = MarkdownStream::new();
        stream.append("plain paragraph\n\n");
        stream.append("footnote[^1] here\n\n");
        let mid = stream.snapshot();
        assert!(mid.blocks.is_empty(), "reset must have cleared the cache");
        assert!(mid.pending.is_some(), "whole document becomes the tail");
        stream.append("more prose\n\n");
        stream.append("[^1]: the note\n");
        stream.finish();
        let snapshot = stream.snapshot();
        assert!(snapshot.pending.is_none());
        assert_eq!(snapshot.blocks.len(), 1);
        assert_eq!(snapshot.blocks[0].block_id, 1);
        assert_eq!(snapshot.blocks[0].version, 1);
        assert!(snapshot.blocks[0].nodes.len() >= 3);
        assert_eq!(stream.total_parse_count(), 1);
    }

    // ---------- 10k 长文内存有界 ----------

    #[test]
    fn ten_thousand_line_document_keeps_cache_bounded_and_linear() {
        let doc = long_doc(10_000);
        let deltas = split_deltas(&doc, 0x5EED);
        let mut stream = MarkdownStream::new();
        for delta in &deltas {
            stream.append(delta);
        }
        stream.finish();
        let snapshot = stream.snapshot();
        assert!(snapshot.pending.is_none());
        assert!(snapshot.blocks.len() > 1_000);
        // 缓存按块冻结：48k+ delta 流中每个 committed 块仍只解析一次
        for block in &snapshot.blocks {
            assert_eq!(stream.parse_count(block.block_id), 1);
        }
        // 内存与文档线性（粗断言）：冻结缓存持有的文本量 ≤ 文档的 10 倍。
        // 若缓存随 delta 数累积，4.8 万 delta 下必然远超此界。
        let cache = stream.cache_bytes();
        assert!(
            cache <= 10 * doc.len(),
            "frozen cache {cache} bytes is not bounded by 10x document size ({})",
            doc.len()
        );
    }
}
