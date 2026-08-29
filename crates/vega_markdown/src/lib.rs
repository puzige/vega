//! Streaming Markdown → render instructions (tech-spec §5, A2-02 / S3-T15).
//!
//! [`MarkdownStream`] wraps `mdstream` 0.3.0's committed+pending block model
//! and converts each block into a pure-data [`RenderNode`] tree via
//! `pulldown-cmark` (GFM tables / tasklists / strikethrough enabled):
//!
//! - **Committed** blocks are parsed exactly once and frozen behind a
//!   per-`BlockId` cache (never re-parsed). `Update.reset` rebuilds the whole
//!   cache; `Update.invalidated` re-parses only the listed blocks (late
//!   reference definitions, resolved through mdstream's `PulldownAdapter`
//!   document-wide definition prelude).
//! - The single **pending** tail block is lightly re-parsed after every
//!   append from mdstream's terminator-completed display view, and never
//!   enters the frozen cache.
//! - [`MarkdownStream::finish`] implements the final semantics (tech-spec
//!   §5.4): the pending completion guess is discarded and the tail block is
//!   re-parsed once from its final raw content, then frozen.
//!
//! [`snapshot`](MarkdownStream::snapshot) hands out a borrowed, ordered view
//! with every block's `BlockId` and parse version, so the UI layer diffs
//! frames by `(block_id, version)` and never re-renders frozen blocks.
//!
//! The crate is UI-free (no gpui, headless like `vega_runtime`): outputs are
//! plain render instructions for the S3-T17/T18 layers to map onto GPUI
//! elements. Delta coalescing / throttling is the caller's job (tech-spec
//! §5.1; T15 keeps the crate synchronous and dependency-light).
//!
//! # Example
//!
//! ```
//! use vega_markdown::{MarkdownStream, RenderNode};
//!
//! let mut stream = MarkdownStream::new();
//! stream.append("# Title\n\nBody ");
//! stream.append("text.\n");
//! stream.finish();
//!
//! let snapshot = stream.snapshot();
//! assert_eq!(snapshot.blocks.len(), 2);
//! assert!(snapshot.pending.is_none());
//! assert!(matches!(
//!     snapshot.blocks[0].nodes,
//!     [RenderNode::Heading { level: 1, .. }]
//! ));
//! assert!(matches!(
//!     snapshot.blocks[1].nodes,
//!     [RenderNode::Paragraph { .. }]
//! ));
//! ```

mod nodes;
mod stream;

pub use nodes::{Inline, ListBlock, ListItem, RenderNode, TableAlignment, TableBlock, TableCell};
pub use stream::{BlockView, MarkdownStream, PendingView, StreamSnapshot};
