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
    MouseUpEvent, Pixels, Render, Rgba, Window, actions, div, point, px, uniform_list,
};
use vega_conversation::agent::PermissionQueue;
use vega_conversation::history::{HistoryEntry, HistoryPage};
use vega_conversation::types::{
    ComposerDefaults, ConversationEvent, ConversationMeter, FileIndexSnapshot, MeterSnapshot,
    PermissionMode, Plan, RestoredUsage, RunUsageEstimator, TaskCostSummary, Thread, ThreadMode,
};
use vega_markdown::{
    BlockView, HighlightKind, HighlightSpan, Inline, ListBlock, MarkdownStream, MockReplay,
    RenderNode, StreamSnapshot, TableAlignment, TableBlock,
};
use vega_theme::{ThemeColors, Typography, theme};

use crate::artifact_card::ArtifactCard;
use crate::branch_selector::BranchSelector;
use crate::commit_panel::CommitPanel;
use crate::file_selector::{
    AcceptFile, CancelFile, FILE_SUGGESTION_LIMIT, FileSelectorModel, NextFile, PreviousFile,
};
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
        OpenWorkspaceDiff,
        ActivateModel,
        PreviousModel,
        NextModel,
        CloseModel,
        CycleThinking
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

/// Bounded `@file` index request (A2-12): the composer selector opened and
/// needs a fresh bounded walk of the project root. The app layer walks on a
/// worker thread and hands back the typed [`FileIndexSnapshot`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileIndexRequested {
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

/// Scroll-up hydration request (S8-T45/C7): the viewport reached the top of
/// the list while older durable history may exist. `before` is the keyset
/// cursor (the oldest loaded `seq`); the app layer reads the page off the UI
/// thread and hands back the typed projection. The stream carries no SQLite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryPageRequested {
    pub thread_id: String,
    pub before: i64,
}

/// Composer `@file` suggestion dropdown open/close/bookkeeping is UI-local;
/// only the accepted completion flows out of the selector model.
///
/// Bounded `@file` selector state (A2-12): pure model + bounded snapshot,
/// driven by the app layer's typed index projection.
///
/// Default provider/model/thinking choice for new threads (A2-14). Emitted
/// on selector activation; the app persists it at the config seam and
/// reflects it back through [`ConversationStream::apply_composer_defaults`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposerDefaultsRequested {
    pub thread_id: String,
    pub defaults: ComposerDefaults,
}

/// Scroll-up hydration bookkeeping. `older_cursor` is `Some(oldest loaded
/// seq)` while older pages may exist and `None` at the durable beginning of
/// the thread (or before any page arrived). One page is in flight at a time;
/// a failed load pauses auto-retry until the viewport leaves the top edge.
mod composer;
mod content;
mod core;
mod model;
mod render;
mod render_rows;

#[cfg(test)]
mod tests;

pub use core::ConversationStream;
pub(crate) use core::*;
pub(crate) use model::*;
pub(crate) use render_rows::*;
