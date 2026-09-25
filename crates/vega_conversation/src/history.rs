//! Typed conversation-history hydration (S8-T45/C7): turns `messages.seq`
//! keyset pages into UI-ready entries. This crate is the redaction boundary
//! the card mandates before anything reaches `vega_ui` — tool inputs reduce
//! through the owner projections ([`tool_card_input_projection`] /
//! [`tool_card_result_projection`]), raw write/edit bodies and audit JSON
//! never leave it, and interrupted/failed rows plus Plan/summary references
//! stay visible (C7 内容完整性).
//!
//! One page load is a bounded constant of store statements: the store reads
//! the message page and the tool-call batch under one read snapshot, then a
//! separately bounded Skill audit/snapshot batch (8 MiB maximum) without
//! source-file reads. The newest page additionally re-projects the S7
//! summary reference (T40 real persisted form: durable `token_usage`/
//! `tool_calls` audits keyed by the terminal assistant message id). No
//! per-message queries anywhere.

use std::collections::HashMap;

use vega_runtime::skills::SourceScope;
use vega_store::Store;
use vega_store::messages::{self, MessagePage, PageCursor, PageRequestError, PageToolCall};
use vega_store::recovery;
use vega_store::skills::{self, SkillActivationAuditRecord};

use crate::types::{
    Approval, ApprovalAudit, ApprovalSource, ConversationError, InvalidToolCode, InvalidToolKind,
    Plan, PlanStatus, TaskCostSummary, ToolCallStatus, ToolCardInputProjection,
    ToolCardResultProjection, ToolResult, tool_card_input_projection, tool_card_result_projection,
};

/// One hydratable conversation entry in ascending message position. R70
/// offsets interleave new tool audits with independently finalized text
/// segments; legacy audits retain their original text-then-tools order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryEntry {
    /// Durable user text (the synthetic approval instruction is controller
    /// capability, not conversation content, and is dropped — same rule as
    /// Composer history).
    UserText {
        seq: i64,
        message_id: String,
        content: String,
    },
    /// Explicit images directly following their owning user text.
    UserImages {
        seq: i64,
        message_id: String,
        images: Vec<crate::types::ImageAttachment>,
    },
    /// One durable assistant text segment with its terminal state on the tail.
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
    /// Content-free, non-executable provenance for one historically loaded
    /// Skill. A durable audit alone never confers current authority.
    SkillActivation {
        seq: i64,
        activation: SkillHistoryActivation,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillHistorySource {
    Project,
    VegaGlobal,
    Imported,
}

impl SkillHistorySource {
    fn from_audit(value: &str) -> Option<Self> {
        match value {
            "project" => Some(Self::Project),
            "vega_global" => Some(Self::VegaGlobal),
            "imported" => Some(Self::Imported),
            _ => None,
        }
    }

    fn matches_runtime(self, value: SourceScope) -> bool {
        matches!(
            (self, value),
            (Self::Project, SourceScope::Project)
                | (Self::VegaGlobal, SourceScope::VegaGlobal)
                | (Self::Imported, SourceScope::Imported)
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillHistoryOrigin {
    Model,
    ExplicitUser,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillHistoryStatus {
    Loaded,
    Revoked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillHistoryVerification {
    /// Store SHA, run binding and frozen inner format all validated, and the
    /// exact activated name/scope/body hash matched this audit.
    Verified,
    /// Snapshot absent, stale, over budget or invalid. Audit remains visible
    /// but is never represented as currently executable authority.
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillHistoryActivation {
    pub run_id: String,
    pub name: String,
    pub source_scope: SkillHistorySource,
    pub content_sha256: String,
    pub origin: SkillHistoryOrigin,
    pub status: SkillHistoryStatus,
    pub verification: SkillHistoryVerification,
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
    if let (Some(oldest), Some(newest)) = (page.rows.first(), page.rows.last()) {
        let records =
            skills::load_history_page_records(store.conn(), thread_id, oldest.seq, newest.seq)
                .map_err(|error| ConversationError::Store(error.to_string()))?;
        let by_run = project_skill_history(records);
        // Insert after each owning assistant's final text/tool segment. This
        // preserves the ordinary user/tool timeline and never creates a fake
        // tool call for an explicitly preloaded Skill.
        for row in page.rows.iter().rev().filter(|row| row.role == "assistant") {
            let Some(activations) = by_run.get(&row.id) else {
                continue;
            };
            let Some(position) = entries.iter().rposition(|entry| {
                matches!(entry,
                    HistoryEntry::AssistantText { message_id, .. }
                    | HistoryEntry::Tool { message_id, .. }
                    if message_id == &row.id)
            }) else {
                continue;
            };
            for activation in activations.iter().rev() {
                entries.insert(
                    position + 1,
                    HistoryEntry::SkillActivation {
                        seq: row.seq,
                        activation: activation.clone(),
                    },
                );
            }
        }
    }
    // S7 summary reference (C7): the thread's latest terminal assistant task
    // re-projects its cost summary from the durable audits exactly like the
    // restart recovery of S7-T40. Attach it only while its message is on this
    // page so the hydrated transcript keeps the card in sequence position.
    if attach_summary
        && let Some(summary) = crate::summary::latest_task_summary(store, thread_id, None)?
        && let Some(position) = entries.iter().rposition(|entry| {
            matches!(
                entry,
                HistoryEntry::AssistantText { message_id, .. }
                    | HistoryEntry::Tool { message_id, .. }
                    if *message_id == summary.message_id
            )
        })
    {
        let assistant_seq = match &entries[position] {
            HistoryEntry::AssistantText { seq, .. } => *seq,
            HistoryEntry::Tool { .. } => page
                .rows
                .iter()
                .find(|row| row.id == summary.message_id)
                .map(|row| row.seq)
                .ok_or_else(|| ConversationError::CorruptRow("summary owner missing".into()))?,
            _ => unreachable!("position matched an owner entry"),
        };
        entries.insert(
            position + 1,
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

fn project_skill_history(
    records: skills::SkillHistoryPageRecords,
) -> HashMap<String, Vec<SkillHistoryActivation>> {
    let validated: HashMap<_, _> = records
        .recoverable_snapshots
        .iter()
        .filter_map(|snapshot| {
            crate::agent::validate_frozen_snapshot(snapshot)
                .map(|activations| (snapshot.run_id.as_str(), activations))
        })
        .collect();
    let mut by_run: HashMap<String, Vec<SkillHistoryActivation>> = HashMap::new();
    for audit in records.audits {
        let Some((scope, hash, origin)) = parsed_skill_audit(&audit) else {
            continue;
        };
        let activations = by_run.entry(audit.run_id.clone()).or_default();
        let existing = activations.iter_mut().find(|activation| {
            activation.name == audit.name
                && activation.source_scope == scope
                && activation.content_sha256 == hash
                && activation.origin == origin
        });
        if audit.status == "revoked" {
            if let Some(existing) = existing {
                existing.status = SkillHistoryStatus::Revoked;
                existing.verification = SkillHistoryVerification::Unavailable;
            }
            continue;
        }
        if existing.is_some() {
            continue;
        }
        let content_sha256 = hash.to_string();
        let verification = if validated.get(audit.run_id.as_str()).is_some_and(|frozen| {
            frozen.iter().any(|skill| {
                skill.name == audit.name
                    && skill.content_sha256 == hash
                    && scope.matches_runtime(skill.source_scope)
            })
        }) {
            SkillHistoryVerification::Verified
        } else {
            SkillHistoryVerification::Unavailable
        };
        activations.push(SkillHistoryActivation {
            run_id: audit.run_id,
            name: audit.name,
            source_scope: scope,
            content_sha256,
            origin,
            status: SkillHistoryStatus::Loaded,
            verification,
        });
    }
    by_run
}

fn parsed_skill_audit(
    audit: &SkillActivationAuditRecord,
) -> Option<(SkillHistorySource, &str, SkillHistoryOrigin)> {
    let scope = SkillHistorySource::from_audit(audit.source_scope.as_deref()?)?;
    let hash = audit.content_sha256.as_deref()?;
    if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let origin = match audit.origin.as_str() {
        "model" => SkillHistoryOrigin::Model,
        "explicit_user" => SkillHistoryOrigin::ExplicitUser,
        _ => return None,
    };
    Some((scope, hash, origin))
}

/// Maps the page rows + batched tool calls into typed entries. Tool calls
/// group onto their owning message (`tool_calls.message_id`; the two tables
/// keep independent seq counters, so ownership is the only reliable join).
/// Any row outside the typed vocabulary fails closed: hydration never
/// silently drops durable content.
fn project_rows(page: &MessagePage) -> Result<Vec<HistoryEntry>, ConversationError> {
    if page.images.iter().any(|image| {
        !page
            .rows
            .iter()
            .any(|row| row.id == image.message_id && row.role == "user")
    }) {
        return Err(ConversationError::CorruptRow("invalid image owner".into()));
    }
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
                    message_id: row.id.clone(),
                    content: row.content.clone(),
                });
                let images = page
                    .images
                    .iter()
                    .filter(|image| image.message_id == row.id)
                    .map(|image| {
                        crate::types::ImageAttachment::from_bytes(image.encoded.clone())
                            .map_err(|error| ConversationError::CorruptRow(error.to_string()))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                crate::attachments::validate_images(&images)
                    .map_err(|error| ConversationError::CorruptRow(error.to_string()))?;
                if !images.is_empty() {
                    entries.push(HistoryEntry::UserImages {
                        seq: row.seq,
                        message_id: row.id.clone(),
                        images,
                    });
                }
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
                    let calls = calls_by_message.remove(row.id.as_str()).unwrap_or_default();
                    project_assistant(row, status, &calls, &mut entries)?;
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

fn project_assistant(
    row: &vega_store::messages::MessageRow,
    status: AssistantStatus,
    calls: &[&PageToolCall],
    entries: &mut Vec<HistoryEntry>,
) -> Result<(), ConversationError> {
    // One NULL makes the whole message legacy. Prior content has no exact
    // boundary, so never infer positions from timestamps or adjacent text.
    if calls.iter().any(|call| call.text_offset_bytes.is_none()) {
        entries.push(HistoryEntry::AssistantText {
            seq: row.seq,
            message_id: row.id.clone(),
            content: row.content.clone(),
            status,
        });
        entries.extend(calls.iter().map(|call| tool_entry(call)));
        return Ok(());
    }
    let mut start = 0;
    for call in calls {
        let offset = call
            .text_offset_bytes
            .and_then(|value| usize::try_from(value).ok())
            .filter(|offset| *offset >= start && row.content.is_char_boundary(*offset))
            .ok_or_else(|| ConversationError::CorruptRow("invalid tool text offset".into()))?;
        if offset > start {
            entries.push(HistoryEntry::AssistantText {
                seq: row.seq,
                message_id: row.id.clone(),
                content: row.content[start..offset].to_string(),
                status: AssistantStatus::Done,
            });
        }
        entries.push(tool_entry(call));
        start = offset;
    }
    if start < row.content.len()
        || calls.is_empty()
        || matches!(
            status,
            AssistantStatus::Failed | AssistantStatus::Interrupted
        )
    {
        entries.push(HistoryEntry::AssistantText {
            seq: row.seq,
            message_id: row.id.clone(),
            content: row.content[start..].to_string(),
            status,
        });
    }
    Ok(())
}

/// Reduces one raw audit row into the owned safe projection. Unknown or
/// corrupt shapes collapse to the fixed content-free card the live path
/// uses (`ToolCard::corrupt`) — hydration must never fabricate plausible
/// content from an unverifiable row.
fn tool_entry(call: &PageToolCall) -> HistoryEntry {
    let status = ToolCallStatus::parse(&call.status).unwrap_or(ToolCallStatus::Failed);
    let approval_audit = call
        .approval
        .as_deref()
        .and_then(|raw| ApprovalAudit::from_json(raw).ok());
    let approval = approval_audit.as_ref().map(|audit| audit.decision);
    let bash_invalid = call.tool == "bash"
        && status == ToolCallStatus::Rejected
        && approval_audit.as_ref().is_some_and(|audit| {
            audit.decision == Approval::Deny
                && audit.source == ApprovalSource::Validation
                && audit.note.is_none()
                && audit.danger.is_none()
        })
        && call.exit_code.is_none()
        && call.duration_ms.is_none()
        && match call.output_text.as_deref() {
            Some(vega_runtime::BASH_INVALID_INPUT_OUTPUT) => {
                vega_runtime::InvalidBashAudit::from_json(&call.input_json).is_some()
            }
            Some(vega_runtime::LEGACY_BASH_INVALID_INPUT_OUTPUT) => {
                vega_tools::bash_permission_signature(&call.input_json).is_err()
            }
            _ => false,
        };
    if bash_invalid {
        return HistoryEntry::Tool {
            seq: call.seq,
            message_id: call.message_id.clone(),
            call_id: call.id.clone(),
            status,
            approval,
            input: None,
            result: Some(ToolCardResultProjection::InvalidRejected {
                tool: InvalidToolKind::Bash,
                code: InvalidToolCode::InvalidInput,
                reused: true,
            }),
        };
    }
    let invalid_bash_claim = call.tool == "bash"
        && (approval_audit
            .as_ref()
            .is_some_and(|audit| audit.source == ApprovalSource::Validation)
            || matches!(
                call.output_text.as_deref(),
                Some(vega_runtime::BASH_INVALID_INPUT_OUTPUT)
                    | Some(vega_runtime::LEGACY_BASH_INVALID_INPUT_OUTPUT)
            ));
    let proposal = crate::types::ToolCall {
        id: call.id.clone(),
        tool: call.tool.clone(),
        input_json: call.input_json.clone(),
    };
    let input = tool_card_input_projection(&proposal);
    let (input, result) = if invalid_bash_claim
        || matches!(input, ToolCardInputProjection::Corrupt)
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

#[cfg(test)]
mod issue90_tests {
    use super::*;

    fn old_invalid_bash_row() -> PageToolCall {
        PageToolCall {
            id: "old-bash".into(),
            message_id: "assistant".into(),
            seq: 1,
            text_offset_bytes: None,
            tool: "bash".into(),
            input_json: r#"{"command":"SECRET_HISTORICAL_COMMAND"}"#.into(),
            output_text: Some("Tool error: invalid bash input (invalid_input)".into()),
            status: "rejected".into(),
            approval: Some(
                r#"{"decision":"deny","note":null,"source":"validation","danger":null}"#.into(),
            ),
            exit_code: None,
            duration_ms: None,
        }
    }

    #[test]
    fn legacy_invalid_bash_hydrates_as_safe_rejection_only_for_exact_validation_row() {
        let row = old_invalid_bash_row();
        assert!(matches!(
            tool_entry(&row),
            HistoryEntry::Tool {
                status: ToolCallStatus::Rejected,
                input: None,
                result: Some(result),
                ..
            } if !matches!(result, ToolCardResultProjection::Corrupt)
        ));

        for tamper in 0..5 {
            let mut row = old_invalid_bash_row();
            match tamper {
                0 => row.status = "failed".into(),
                1 => {
                    row.approval = Some(
                        r#"{"decision":"deny","note":null,"source":"user","danger":null}"#.into(),
                    )
                }
                2 => row.exit_code = Some(0),
                3 => row.output_text = Some("SECRET_FORGED_OUTPUT".into()),
                _ => row.input_json = r#"{"cmd":"printf valid"}"#.into(),
            }
            assert!(matches!(
                tool_entry(&row),
                HistoryEntry::Tool {
                    result: Some(ToolCardResultProjection::Corrupt),
                    ..
                }
            ));
        }
    }
}
