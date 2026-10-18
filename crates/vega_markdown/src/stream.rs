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
use mdstream::{BlockKind, MdStream, Options, ReferenceDefinitionsMode, Update};
use pulldown_cmark::Options as PulldownOptions;

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
        }

        // pending 尾块：每次 append 后按 terminator 补全视图轻量重解析，不入冻结缓存
        match update.pending {
            Some(mut pending) => {
                if pending.kind == BlockKind::CodeFence {
                    pending.display = None;
                }
                let events = self.adapter.parse_pending(&pending);
                let mut nodes = render_nodes_from_events(&events);
                if pending
                    .display
                    .as_deref()
                    .is_some_and(|display| display != pending.raw.as_str())
                    && crate::nodes::contains_code(&nodes)
                {
                    pending.display = None;
                    let raw_events = self.adapter.parse_pending(&pending);
                    let raw_nodes = render_nodes_from_events(&raw_events);
                    crate::nodes::restore_code(&mut nodes, &raw_nodes);
                }
                let version = match self.pending.as_ref() {
                    Some(previous) if previous.block_id == pending.id.0 => previous.version + 1,
                    _ => 1,
                };
                self.pending = Some(PendingEntry {
                    block_id: pending.id.0,
                    version,
                    nodes,
                });
            }
            None => self.pending = None,
        }
    }
}
