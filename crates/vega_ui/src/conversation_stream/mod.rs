mod privacy;
pub use privacy::{PrivacyAction, PrivacyRequested};
mod attachments;
mod ordinary_submissions;
#[cfg(test)]
mod ordinary_tests;
mod pending_messages;
pub use ordinary_submissions::{OrdinaryAction, OrdinaryActionRequested};
mod user_questions;
pub use pending_messages::{PendingQueueAction, PendingQueueRequested, RunningMessageDefault};
pub mod bench;
mod computer_use;
mod network_retry;
mod screenshots;
mod skills;
use vega_conversation::types::ImageAttachment;

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui_kit::prelude::*;
use gpui_kit::{
    AbsoluteLength, AnyElement, App, Context, DefiniteLength, Entity, EventEmitter, FocusHandle,
    FontStyle, FontWeight, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Render, Rgba,
    ScrollHandle, StrikethroughStyle, StyledText, TextRun, TextStyle, Window, actions, div, list,
    px,
};
use vega_conversation::agent::{PendingPermission, PermissionQueue};
use vega_conversation::history::{HistoryEntry, HistoryPage};
use vega_conversation::types::{
    ComposerDefaults, ConversationEvent, ConversationMeter, FileIndexSnapshot, FrozenReasoning,
    PendingMessageBehavior, PendingQueueProjection, PermissionMode, Plan, ProviderPreflightFailure,
    ReasoningChoice, ReasoningProfileProjection, ReasoningSupport, RestoredUsage, RunFailureKind,
    RunUsageEstimator, SkillComposerMutation, SkillComposerProjection, SkillSelectionIntent,
    TaskBackend, TaskCostSummary, Thread, ThreadMode,
};
use vega_markdown::{
    BlockView, HighlightKind, HighlightSpan, Inline, ListBlock, MarkdownStream, MockReplay,
    RenderNode, StreamSnapshot, TableAlignment, TableBlock,
};
use vega_theme::{Layout, ThemeColors, Typography, theme};

use crate::artifact_card::{ArtifactCard, ArtifactGroup};
use crate::branch_selector::{BranchListRequested, BranchSelector};
use crate::commit_panel::CommitPanel;
use crate::file_selector::{
    AcceptFile, CancelFile, FILE_SUGGESTION_LIMIT, FileSelectorModel, FocusPreviousRetry,
    FocusRetry, NextFile, PreviousFile, RetryFile,
};
use crate::permission_card::{PermissionCard, PermissionCardResolved};
use crate::plan_card::{PlanCard, PlanReviewRequested};
use crate::settings::SettingsOpen;
use crate::summary_card::SummaryCard;
use crate::text_input::TextInput;
use crate::tool_activity_group::ToolActivityGroup;
use crate::tool_card::ToolCard;

// The payload is a shared conversation boundary; keep this re-export so
// existing UI callers retain the same import path while the owning type lives
// in `vega_conversation::types`.
pub use vega_conversation::types::ThreadModelSelectionRequested;

actions!(
    vega_conversation_stream,
    [
        SendMessage,
        PreviousMessage,
        OpenWorkspaceDiff,
        ActivateModel,
        PreviousModel,
        NextModel,
        CloseModel,
        ResumeTail,
        PreviousComposerAction,
        NextComposerAction,
        AcceptComposerAction,
        CloseComposerActions,
        StopComposer,
        NextComposerControl,
        PreviousComposerControl,
        ActivatePendingMessageControl,
        ClosePendingMessageMenu,
        NextPendingMessageMenuItem,
        CopySelectedText
    ]
);

// Shared result vocabularies remain owned by `vega_conversation::types`; keep
// the old UI import path for callers that render or test the selector.
pub use vega_conversation::types::{FileIndexFailureCode, FileReferenceFailureCode};

/// Distance from the bottom (px) below which the view still counts as pinned
/// (native tail-follow resume tolerance; mirrors the list's own 1px bottom
/// test and the old anchor machine's epsilon).
pub(crate) const ANCHOR_EPSILON_PX: f32 = 1.0;

/// Compact-subrow height (logical px, C4 rule 1): diff lines, card-internal
/// lines and other genuinely uniform secondary rows. The top-level
/// conversation list is variable-height and never uses this value.
pub(crate) const ROW_HEIGHT: f32 = 24.0;

/// Demo injection pacing: the 16ms tick polls; the injected count follows
/// `INJECT_RATE × elapsed` (≈500 δ/s 任务卡口径，自校正抵消主线程抖动).
const INJECT_TICK: Duration = Duration::from_millis(16);
const INJECT_RATE: usize = 500;

/// Composer starts at one visible row and grows to the TextInput's 1–8 row
/// viewport as content changes (ui-spec §4.4).
const COMPOSER_ROWS: usize = 1;

pub struct ImagePreviewRequested {
    pub image: Entity<crate::image_preview::ImagePreview>,
    pub action: crate::image_preview::ImagePreviewAction,
}

/// Typed settings request emitted upward; persistence remains in conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadSettingsRequested {
    pub thread_id: String,
    pub mode: Option<ThreadMode>,
    pub permission_mode: Option<PermissionMode>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskBackendSelectionRequested {
    pub thread_id: String,
    pub backend: TaskBackend,
}

/// Composer submission routed to the application controller.
#[derive(Clone, PartialEq, Eq)]
pub struct ComposerSubmitted {
    pub request_id: u64,
    pub draft_revision: u64,
    pub attachment_ids: Vec<u64>,
    pub pending_behavior: Option<PendingMessageBehavior>,
    pub images: Vec<ImageAttachment>,
    pub thread_id: String,
    pub content: String,
    pub skill_intent: Option<SkillSelectionIntent>,
    /// Run-start reasoning snapshot captured from the displayed exact
    /// provider/model projection. Raw reasoning text is never part of this
    /// event.
    /// An invalid explicit profile is an error, rather than an implicit
    /// provider-default fallback. The app controller can reject the submit
    /// while keeping the input draft intact.
    pub reasoning: Result<Option<FrozenReasoning>, ReasoningSubmitError>,
}

impl std::fmt::Debug for ComposerSubmitted {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ComposerSubmitted")
            .field("request_id", &self.request_id)
            .finish_non_exhaustive()
    }
}

/// Read-only, bounded Skills catalog request for this exact route.
#[derive(Clone, PartialEq, Eq)]
pub struct SkillComposerProjectionRequested {
    pub thread_id: String,
    pub project_id: String,
    pub draft: bool,
    pub generation: u64,
}

/// One persisted-thread UI gesture; no filesystem or Store IO in the view.
#[derive(Clone, PartialEq, Eq)]
pub struct SkillComposerMutationRequested {
    pub thread_id: String,
    pub project_id: String,
    pub owner_generation: u64,
    pub expected_generation: u64,
    pub mutation: SkillComposerMutation,
}

/// Reasons a run-start thinking snapshot cannot be submitted. The enum keeps
/// failure typed without carrying provider data or raw reasoning content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReasoningSubmitError {
    /// The independent capability authority is unavailable or unresolved.
    Unavailable,
    /// The displayed declaration or preference violates its exact profile.
    Invalid,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileIndexRequested {
    pub thread_id: String,
    pub project_id: String,
    pub generation: u64,
}

/// Cancels the current bounded index when the token is closed or the input
/// leaves an `@` query. The app layer cancels the matching worker and fences
/// its result; the stream keeps the generation monotonic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileIndexCancelled {
    pub thread_id: String,
    pub project_id: String,
    pub generation: u64,
}

/// Starts a fresh bounded index after a visible failure. A retry always uses a
/// new generation so a late result from the failed job cannot land.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileIndexRetryRequested {
    pub thread_id: String,
    pub project_id: String,
    pub generation: u64,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryPageRequested {
    pub thread_id: String,
    pub before: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewerHistoryPageRequested {
    pub thread_id: String,
    pub after: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MessageLocationRequested {
    pub thread_id: String,
    pub message_id: String,
    pub restore_anchor: Option<ThreadScrollAnchor>,
}

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
mod composer_predictions;
pub(crate) use composer_predictions::ComposerPredictionsEnabled;
pub use composer_predictions::set_composer_predictions_enabled;
mod composer_actions;
use composer_actions::ComposerActions;
mod code_drag;
mod code_viewport;
mod content;
mod context_control;
mod copy_receipt;
pub use context_control::ContextUsageProjection;
mod context_usage;
pub use vega_conversation::types::{
    ContextCompactionCancelRequested, ContextCompactionRequested, ContextSettingsRequested,
    MessageLocationStatus,
};
mod core;
mod file_reference;
mod message_anchors;
mod model;
mod phase;
mod render;
mod render_rows;
mod run_activity;
mod run_notice;
mod shimmer;
mod sticky_disclosure;
mod subagents;
mod text_cache;
use run_activity::{RunActivityChild, RunActivityGroup, RunActivityStatus};
mod autolinks;
mod inline_code;
pub(crate) mod links;
mod selection;
mod selection_menu;
mod thinking;
mod thinking_slider;
use thinking::ThinkingBlock;
mod utility_bar;

pub use core::ConversationStream;
pub use core::ModelPickerLevel;
pub(crate) use core::*;
pub(crate) use model::*;
pub(crate) use render_rows::*;
pub use thinking_slider::{
    OFF_CHOICE_NAME, OFF_LABEL, PROVIDER_DEFAULT_LABEL, ThinkingSlider, ThinkingSliderModel,
    ThinkingSliderTitleActivated, ThinkingTierSelected, resolve_tier, track_fill_style,
};
pub use vega_conversation::types::ThreadScrollAnchor;

/// Explicit stop for the currently owned worker; the app keeps its generation until terminal.
pub struct ComposerStopRequested {
    pub thread_id: String,
}

pub struct CredentialContinuationRequested {
    pub thread_id: String,
    pub message_id: String,
}

mod automation;
pub use automation::{AutomationEditorRequested, AutomationRequested, AutomationTiming};
