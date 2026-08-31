//! Typed conversation-history hydration (S8-T45/C7): turns `messages.seq`
//! keyset pages into UI-ready entries. This crate is the redaction boundary
//! the card mandates before anything reaches `vega_ui` — tool inputs reduce
//! through the owner projections ([`tool_card_input_projection`] /
//! [`tool_card_result_projection`]), raw write/edit bodies and audit JSON
//! never leave it, and interrupted/failed rows plus Plan/summary references
//! stay visible (C7 内容完整性).
//!
//! One page load is a bounded constant of store statements: the store reads
//! the message page and the tool-call batch under one read snapshot, and the
//! newest page additionally re-projects the S7 summary reference (T40 real
//! persisted form: durable `token_usage`/`tool_calls` audits keyed by the
//! terminal assistant message id). No per-message queries anywhere.

use std::collections::HashMap;

use vega_store::Store;
use vega_store::messages::{self, MessagePage, PageCursor, PageRequestError, PageToolCall};
use vega_store::recovery;

use crate::types::{
    Approval, ApprovalAudit, ConversationError, Plan, PlanStatus, TaskCostSummary, ToolCallStatus,
    ToolCardInputProjection, ToolCardResultProjection, ToolResult, tool_card_input_projection,
    tool_card_result_projection,
};

/// One hydratable conversation entry in ascending seq position. Tool cards
/// attach directly after their owning assistant message in persisted
/// call-seq order; the durable model cannot reconstruct the exact mid-stream
/// interleaving, so this fixed adjacency is the hydration contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryEntry {
    /// Durable user text (the synthetic approval instruction is controller
    /// capability, not conversation content, and is dropped — same rule as
    /// Composer history).
    UserText { seq: i64, content: String },
    /// Durable assistant text turn with its terminal state.
    AssistantText {
        seq: i64,
        message_id: String,
        content: String,
        status: AssistantStatus,
    },
    /// Durable Plan card with its review state.
    Plan { seq: i64, plan: Plan },
    /// Read-only S7 per-task cost summary reference, attached directly after
    /// the assistant message (and its tools) it summarizes.
    Summary { seq: i64, summary: TaskCostSummary },
    /// Audited tool call reduced to the safe UI projection. `call_id` is an
    /// opaque map key (never rendered), matching the live event shape.
    Tool {
        seq: i64,
        message_id: String,
        call_id: String,
        status: ToolCallStatus,
        approval: Option<Approval>,
        input: Option<ToolCardInputProjection>,
        result: Option<ToolCardResultProjection>,
    },
}

/// Terminal assistant vocabulary a hydrated turn may carry (`streaming` rows
/// are not durable and never reach a page; startup repair normalizes them).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssistantStatus {
    Done,
    Interrupted,
    Failed,
}

impl AssistantStatus {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "done" => Some(Self::Done),
            "interrupted" => Some(Self::Interrupted),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

/// Result of one hydration projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryPage {
    /// Entries in ascending seq position (oldest → newest).
    pub entries: Vec<HistoryEntry>,
    /// `Some(oldest_seq)` when older history may exist — pass
    /// [`PageCursor::Before`] into [`history_page_before`]. `None` marks the
    /// durable beginning of the thread.
    pub older_cursor: Option<i64>,
    /// Highest durable `seq` seen on the newest page (`None` for an empty
    /// thread); the UI fences late pages against it after route switches.
    pub newest_seq: Option<i64>,
}

/// Projects the newest page after one startup repair pass (restart entry):
/// the controller is rebuilt first, `recover_thread` normalizes rows the
/// killed process left incomplete, and only then is the page projected.
pub fn restart_history_page(
    store: &Store,
    thread_id: &str,
    limit: usize,
) -> Result<HistoryPage, ConversationError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or_default();
    recovery::recover_thread(store.conn(), thread_id, now)
        .map_err(|error| ConversationError::Store(error.to_string()))?;
    latest_history_page(store, thread_id, limit)
}

fn page_read(
    store: &Store,
    thread_id: &str,
    cursor: PageCursor,
    limit: usize,
) -> Result<MessagePage, ConversationError> {
    messages::page_before(store.conn(), thread_id, cursor, limit).map_err(page_failure)
}

/// Flattens a page-request failure with its full `source` chain (this crate
/// intentionally does not depend on the store's SQLite backend directly, so
/// the cause is recovered through the standard trait). A row the store's own
/// read validator rejected (schema drift, DDL bypass) stays a fail-closed
/// [`ConversationError::CorruptRow`], not a bare IO failure.
fn page_failure(error: PageRequestError) -> ConversationError {
    let mut message = error.to_string();
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(&error);
    while let Some(cause) = source.and_then(std::error::Error::source) {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = Some(cause);
    }
    if message.contains("corrupt") {
        ConversationError::CorruptRow(message)
    } else {
        ConversationError::Store(message)
    }
}

/// Projects the newest page of `thread_id` (in-process route opens that do
/// not need another repair pass). `limit` must be within `1..=200`.
pub fn latest_history_page(
    store: &Store,
    thread_id: &str,
    limit: usize,
) -> Result<HistoryPage, ConversationError> {
    let page = page_read(store, thread_id, PageCursor::Head, limit)?;
    assemble(store, thread_id, page, limit, true)
}

/// Projects the next older page below `cursor` (scroll-up hydration). Pure
/// read: no repair, no summary re-projection — the summary reference belongs
/// to the newest page where its message lives.
pub fn history_page_before(
    store: &Store,
    thread_id: &str,
    cursor: PageCursor,
    limit: usize,
) -> Result<HistoryPage, ConversationError> {
    let page = page_read(store, thread_id, cursor, limit)?;
    assemble(store, thread_id, page, limit, false)
}

fn assemble(
    store: &Store,
    thread_id: &str,
    page: MessagePage,
    limit: usize,
    attach_summary: bool,
) -> Result<HistoryPage, ConversationError> {
    if limit == 0 || limit > messages::PAGE_LIMIT {
        return Err(ConversationError::Store(format!(
            "page size {limit} is outside the 1..=200 page contract"
        )));
    }
    let newest_seq = page.rows.last().map(|row| row.seq);
    let mut entries = project_rows(&page)?;
    // S7 summary reference (C7): the thread's latest terminal assistant task
    // re-projects its cost summary from the durable audits exactly like the
    // restart recovery of S7-T40. Attach it only while its message is on this
    // page so the hydrated transcript keeps the card in sequence position.
    if attach_summary
        && let Some(summary) = crate::summary::latest_task_summary(store, thread_id, None)?
        && let Some(position) = entries.iter().position(|entry| {
            matches!(
                entry,
                HistoryEntry::AssistantText { message_id, .. }
                    if *message_id == summary.message_id
            )
        })
    {
        let assistant_seq = match &entries[position] {
            HistoryEntry::AssistantText { seq, .. } => *seq,
            _ => unreachable!("position matched an assistant entry"),
        };
        // Skip the owner's attached tool cards: the summary aggregates
        // the whole task and belongs after them.
        let mut insert_at = position + 1;
        while insert_at < entries.len() && matches!(&entries[insert_at], HistoryEntry::Tool { .. })
        {
            insert_at += 1;
        }
        entries.insert(
            insert_at,
            HistoryEntry::Summary {
                seq: assistant_seq,
                summary,
            },
        );
    }
    Ok(HistoryPage {
        entries,
        older_cursor: page.older_cursor,
        newest_seq,
    })
}

/// Maps the page rows + batched tool calls into typed entries. Tool calls
/// group onto their owning message (`tool_calls.message_id`; the two tables
/// keep independent seq counters, so ownership is the only reliable join).
/// Any row outside the typed vocabulary fails closed: hydration never
/// silently drops durable content.
fn project_rows(page: &MessagePage) -> Result<Vec<HistoryEntry>, ConversationError> {
    let mut calls_by_message: HashMap<&str, Vec<&PageToolCall>> = HashMap::new();
    for call in &page.tool_calls {
        calls_by_message
            .entry(call.message_id.as_str())
            .or_default()
            .push(call);
    }
    for calls in calls_by_message.values_mut() {
        calls.sort_by_key(|call| call.seq);
    }
    let mut entries: Vec<HistoryEntry> = Vec::new();
    for row in &page.rows {
        match row.role.as_str() {
            "user" => {
                if row.kind != "text" || row.status != "done" {
                    return Err(ConversationError::CorruptRow(format!(
                        "user row seq={}: kind={}/status={}",
                        row.seq, row.kind, row.status
                    )));
                }
                if row.content == crate::plans::APPROVAL_INSTRUCTION {
                    // Synthetic approval instruction: controller capability,
                    // not user-typed content (Composer history rule).
                    continue;
                }
                entries.push(HistoryEntry::UserText {
                    seq: row.seq,
                    content: row.content.clone(),
                });
            }
            "assistant" => match row.kind.as_str() {
                "plan" => {
                    let raw_status = row.plan_status.as_deref().ok_or_else(|| {
                        ConversationError::CorruptRow("completed plan lacks status".to_string())
                    })?;
                    let status = PlanStatus::parse(raw_status).ok_or_else(|| {
                        ConversationError::CorruptRow(
                            "plan status is outside vocabulary".to_string(),
                        )
                    })?;
                    entries.push(HistoryEntry::Plan {
                        seq: row.seq,
                        plan: Plan {
                            id: row.id.clone(),
                            thread_id: row.thread_id.clone(),
                            content: row.content.clone(),
                            status,
                            review_note: row.plan_review_note.clone(),
                            reviewed_at: row.plan_reviewed_at,
                        },
                    });
                }
                _ => {
                    let status = AssistantStatus::parse(&row.status).ok_or_else(|| {
                        ConversationError::CorruptRow(format!(
                            "assistant message status: {}",
                            row.status
                        ))
                    })?;
                    entries.push(HistoryEntry::AssistantText {
                        seq: row.seq,
                        message_id: row.id.clone(),
                        content: row.content.clone(),
                        status,
                    });
                }
            },
            other => {
                return Err(ConversationError::CorruptRow(format!(
                    "page row role: {other}"
                )));
            }
        }
        if let Some(calls) = calls_by_message.remove(row.id.as_str()) {
            for call in calls {
                entries.push(tool_entry(call));
            }
        }
    }
    if let Some(orphan) = calls_by_message.values().next() {
        let orphan = orphan[0];
        return Err(ConversationError::CorruptRow(format!(
            "page tool call {} has no owner message on the page",
            orphan.id
        )));
    }
    Ok(entries)
}

/// Reduces one raw audit row into the owned safe projection. Unknown or
/// corrupt shapes collapse to the fixed content-free card the live path
/// uses (`ToolCard::corrupt`) — hydration must never fabricate plausible
/// content from an unverifiable row.
fn tool_entry(call: &PageToolCall) -> HistoryEntry {
    let status = ToolCallStatus::parse(&call.status).unwrap_or(ToolCallStatus::Failed);
    let approval = call
        .approval
        .as_deref()
        .and_then(|raw| ApprovalAudit::from_json(raw).ok())
        .map(|audit| audit.decision);
    let proposal = crate::types::ToolCall {
        id: call.id.clone(),
        tool: call.tool.clone(),
        input_json: call.input_json.clone(),
    };
    let input = tool_card_input_projection(&proposal);
    let (input, result) = if matches!(input, ToolCardInputProjection::Corrupt)
        || matches!(
            status,
            ToolCallStatus::PendingApproval | ToolCallStatus::Approved | ToolCallStatus::Running
        ) {
        // Corrupt audit rows and non-terminal durable rows render the fixed
        // content-free card; the latter can never be re-driven in the UI.
        (None, Some(ToolCardResultProjection::Corrupt))
    } else {
        let result = ToolResult {
            status,
            output: call.output_text.clone().unwrap_or_default(),
            reused: true,
            exit_code: call.exit_code,
            duration_ms: call.duration_ms.and_then(|ms| u64::try_from(ms).ok()),
            truncated: None,
            invalid: None,
        };
        let projection = tool_card_result_projection(Some(&input), &result);
        (Some(input), Some(projection))
    };
    HistoryEntry::Tool {
        seq: call.seq,
        message_id: call.message_id.clone(),
        call_id: call.id.clone(),
        status,
        approval,
        input,
        result,
    }
}
