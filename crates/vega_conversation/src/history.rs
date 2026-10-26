use std::collections::HashMap;

use vega_runtime::skills::SourceScope;
use vega_store::Store;
use vega_store::messages::{self, MessagePage, PageCursor, PageRequestError, PageToolCall};
use vega_store::recovery;
use vega_store::skills::{self, SkillActivationAuditRecord};

use crate::types::{
    Approval, ApprovalAudit, ApprovalSource, CodexDisplayMetadata, ConversationError,
    InvalidToolCode, InvalidToolKind, Plan, PlanStatus, TaskCostSummary, ToolCallStatus,
    ToolCardInputProjection, ToolCardResultProjection, ToolResult, tool_card_input_projection,
    tool_card_result_projection,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryEntry {
    RunContinuation {
        previous_assistant_id: String,
        user_message_id: String,
        next_assistant_id: String,
    },
    CredentialContinuation {
        seq: i64,
        message_id: String,
        failure: crate::types::RunFailureKind,
    },
    AssistantScreenshot {
        seq: i64,
        message_id: String,
        call_id: String,
        image: crate::types::ImageAttachment,
    },
    AssistantResponseMetadata {
        seq: i64,
        message_id: String,
        response: crate::types::AssistantResponseMetadata,
    },
    GeneratedNotice {
        seq: i64,
        message_id: String,
        notice: crate::types::GeneratedNoticeMetadata,
        display_text_start_bytes: usize,
        display_text_end_bytes: usize,
    },
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
        execution_duration_ms: Option<u64>,
    },
    /// Durable Plan card with its review state.
    Plan {
        seq: i64,
        plan: Plan,
        execution_duration_ms: Option<u64>,
    },
    Summary {
        seq: i64,
        summary: TaskCostSummary,
    },
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
    pub interrupted_tail: Option<crate::types::InterruptedAssistant>,
    pub subagents: Vec<crate::types::SubagentSnapshot>,
    /// Entries in ascending seq position (oldest → newest).
    pub entries: Vec<HistoryEntry>,
    /// `Some(oldest_seq)` when older history may exist — pass
    /// [`PageCursor::Before`] into [`history_page_before`]. `None` marks the
    /// durable beginning of the thread.
    pub older_cursor: Option<i64>,
    pub newer_cursor: Option<i64>,
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
    guard_legacy_history(store, thread_id)?;
    let admission = vega_store::thread_admission::try_acquire(store.database_identity(), thread_id)
        .map_err(|_| {
            ConversationError::Store(vega_i18n::t("domain.ordinary_send.access_unavailable").into())
        })?;
    let Some(_admission) = admission else {
        return latest_history_page(store, thread_id, limit);
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or_default();
    crate::subagents::recover_children(store, thread_id, now)?;
    if !crate::subagents::is_live(store, thread_id) {
        recovery::recover_thread(store.conn(), thread_id, now)
            .map_err(|error| ConversationError::Store(error.to_string()))?;
    }
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
    guard_legacy_history(store, thread_id)?;
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
    guard_legacy_history(store, thread_id)?;
    let page = page_read(store, thread_id, cursor, limit)?;
    assemble(store, thread_id, page, limit, false)
}

pub fn history_page_containing_message(
    store: &Store,
    thread_id: &str,
    message_id: &str,
    limit: usize,
) -> Result<Option<HistoryPage>, ConversationError> {
    guard_legacy_history(store, thread_id)?;
    let page = messages::page_containing_message(store.conn(), thread_id, message_id, limit)
        .map_err(page_failure)?;
    let Some(page) = page else {
        return Ok(None);
    };
    assemble(store, thread_id, page, limit, true).map(Some)
}

pub fn history_page_after(
    store: &Store,
    thread_id: &str,
    cursor: i64,
    limit: usize,
) -> Result<HistoryPage, ConversationError> {
    guard_legacy_history(store, thread_id)?;
    let page =
        messages::page_after(store.conn(), thread_id, cursor, limit).map_err(page_failure)?;
    assemble(store, thread_id, page, limit, true)
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
    let mut page = page;
    let generated_notices = project_page_privacy(store, thread_id, &mut page)?;
    let newest_seq = page.rows.last().map(|row| row.seq);
    let mut entries = project_rows(&page, &generated_notices)?;
    append_skill_history(store, thread_id, &page, &mut entries)?;

    append_task_summary(store, thread_id, &page, &mut entries, attach_summary)?;
    for row in page.rows.iter().filter(|row| row.role == "assistant") {
        if let Some(receipt) = vega_store::credential_continuation::find(store.conn(), &row.id)
            .map_err(|error| ConversationError::Store(error.to_string()))?
        {
            let failure = if receipt.reason == "source_changed" {
                crate::types::RunFailureKind::CredentialContinuationUnavailable
            } else if receipt.state == "unknown" {
                crate::types::RunFailureKind::CredentialOutcomeUnknown
            } else if receipt.state == "ready" {
                match receipt.reason.as_str() {
                    "unavailable" => crate::types::RunFailureKind::CredentialCheckUnavailable,
                    reason if reason.starts_with("projection:") => reason
                        .strip_prefix("projection:")
                        .and_then(|value| value.parse().ok())
                        .map(crate::types::RunFailureKind::CredentialProjectionInvalid)
                        .unwrap_or(crate::types::RunFailureKind::CredentialExposureBlocked),
                    _ => crate::types::RunFailureKind::CredentialExposureBlocked,
                }
            } else {
                crate::types::RunFailureKind::Persisted
            };
            entries.push(HistoryEntry::CredentialContinuation {
                seq: row.seq,
                message_id: row.id.clone(),
                failure,
            });
        }
    }
    Ok(HistoryPage {
        interrupted_tail: interrupted_tail(store, thread_id)?,
        subagents: crate::subagents::history(store, thread_id)?
            .into_iter()
            .filter(|agent| {
                page.rows
                    .iter()
                    .any(|message| message.id == agent.parent_message_id)
            })
            .collect(),
        entries,
        older_cursor: page.older_cursor,
        newer_cursor: page.newer_cursor,
        newest_seq,
    })
}

fn append_task_summary(
    store: &Store,
    thread_id: &str,
    page: &MessagePage,
    entries: &mut Vec<HistoryEntry>,
    attach_summary: bool,
) -> Result<(), ConversationError> {
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
    Ok(())
}

fn append_skill_history(
    store: &Store,
    thread_id: &str,
    page: &MessagePage,
    entries: &mut Vec<HistoryEntry>,
) -> Result<(), ConversationError> {
    if let (Some(oldest), Some(newest)) = (page.rows.first(), page.rows.last()) {
        let records =
            skills::load_history_page_records(store.conn(), thread_id, oldest.seq, newest.seq)
                .map_err(|error| ConversationError::Store(error.to_string()))?;
        let by_run = project_skill_history(records);
        for row in page.rows.iter().rev().filter(|row| row.role == "assistant") {
            let Some(activations) = by_run.get(&row.id) else {
                continue;
            };
            let Some(position) = entries.iter().rposition(|entry| match entry {
                HistoryEntry::AssistantText { message_id, .. }
                | HistoryEntry::Tool { message_id, .. } => message_id == &row.id,
                HistoryEntry::Plan { plan, .. } => plan.id == row.id,
                _ => false,
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

    Ok(())
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
fn project_rows(
    page: &MessagePage,
    generated_notices: &[HistoryEntry],
) -> Result<Vec<HistoryEntry>, ConversationError> {
    project_rows_with(page, generated_notices, &tool_entry)
}

fn project_rows_with(
    page: &MessagePage,
    generated_notices: &[HistoryEntry],
    project_tool: &impl Fn(&PageToolCall) -> HistoryEntry,
) -> Result<Vec<HistoryEntry>, ConversationError> {
    if page.images.iter().any(|image| {
        !page.rows.iter().any(|row| {
            row.id == image.message_id
                && (row.role == "user" && image.artifact_call_id.is_none()
                    || row.role == "assistant"
                        && image.artifact_call_id.as_ref().is_some_and(|id| {
                            page.tool_calls.iter().any(|call| {
                                &call.id == id
                                    && call.message_id == row.id
                                    && call.tool == "ComputerScreenshot"
                            })
                        }))
        })
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
    let mut entries: Vec<HistoryEntry> = page
        .continuations
        .iter()
        .map(|relation| HistoryEntry::RunContinuation {
            previous_assistant_id: relation.previous_assistant_id.clone(),
            user_message_id: relation.user_message_id.clone(),
            next_assistant_id: relation.next_assistant_id.clone(),
        })
        .collect();
    for row in &page.rows {
        entries.extend(generated_notices.iter().filter(|entry| {
            matches!(entry, HistoryEntry::GeneratedNotice { message_id, .. } if message_id == &row.id)
        }).cloned());
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
                        execution_duration_ms: page
                            .execution_durations_ms
                            .get(&row.id)
                            .and_then(|duration| u64::try_from(*duration).ok()),
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
                    for response in page
                        .assistant_responses
                        .iter()
                        .filter(|response| response.message_id == row.id)
                    {
                        let kind = match response.kind.as_str() {
                            "commentary" => crate::types::AssistantResponseKind::Commentary,
                            "final" if status == AssistantStatus::Done => {
                                crate::types::AssistantResponseKind::Final
                            }
                            _ => continue,
                        };
                        let response = crate::types::AssistantResponseMetadata {
                            response_id: response.response_id.clone(),
                            text_start_bytes: response.text_start_bytes,
                            text_end_bytes: response.text_end_bytes,
                            kind,
                        };
                        if response.is_valid_for(&row.content) {
                            entries.push(HistoryEntry::AssistantResponseMetadata {
                                seq: row.seq,
                                message_id: row.id.clone(),
                                response,
                            });
                        }
                    }
                    project_assistant(
                        row,
                        status,
                        page.execution_durations_ms.get(&row.id).copied(),
                        &calls,
                        &mut entries,
                        project_tool,
                    )?;
                }
            },
            other => {
                return Err(ConversationError::CorruptRow(format!(
                    "page row role: {other}"
                )));
            }
        }
        if row.role == "assistant" {
            let mut screenshots = Vec::new();
            for image in page
                .images
                .iter()
                .filter(|image| image.message_id == row.id)
            {
                let attachment =
                    crate::types::ImageAttachment::from_bytes(image.encoded.clone())
                        .map_err(|error| ConversationError::CorruptRow(error.to_string()))?;
                screenshots.push(attachment.clone());
                let call_id = image.artifact_call_id.clone().ok_or_else(|| {
                    ConversationError::CorruptRow("invalid screenshot owner".into())
                })?;
                entries.push(HistoryEntry::AssistantScreenshot {
                    seq: row.seq,
                    message_id: row.id.clone(),
                    call_id,
                    image: attachment,
                });
            }
            crate::attachments::validate_images(&screenshots)
                .map_err(|error| ConversationError::CorruptRow(error.to_string()))?;
        }
        if let Some(calls) = calls_by_message.remove(row.id.as_str()) {
            for call in calls {
                entries.push(project_tool(call));
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
    execution_duration_ms: Option<i64>,
    calls: &[&PageToolCall],
    entries: &mut Vec<HistoryEntry>,
    project_tool: &impl Fn(&PageToolCall) -> HistoryEntry,
) -> Result<(), ConversationError> {
    // One NULL makes the whole message legacy. Prior content has no exact
    // boundary, so never infer positions from timestamps or adjacent text.
    if calls.iter().any(|call| call.text_offset_bytes.is_none()) {
        entries.push(HistoryEntry::AssistantText {
            seq: row.seq,
            message_id: row.id.clone(),
            content: row.content.clone(),
            status,
            execution_duration_ms: execution_duration_ms
                .and_then(|duration| u64::try_from(duration).ok()),
        });
        entries.extend(calls.iter().map(|call| project_tool(call)));
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
                execution_duration_ms: None,
            });
        }
        entries.push(project_tool(call));
        start = offset;
    }
    if start < row.content.len()
        || calls.is_empty()
        || matches!(
            status,
            AssistantStatus::Failed | AssistantStatus::Interrupted
        )
        || execution_duration_ms.is_some()
    {
        entries.push(HistoryEntry::AssistantText {
            seq: row.seq,
            message_id: row.id.clone(),
            content: row.content[start..].to_string(),
            status,
            execution_duration_ms: execution_duration_ms
                .and_then(|duration| u64::try_from(duration).ok()),
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
            Some(output)
                if vega_runtime::InvalidBashAudit::from_json(&call.input_json)
                    .is_some_and(|audit| audit.matches_feedback(output)) =>
            {
                true
            }
            Some(vega_runtime::LEGACY_BASH_INVALID_INPUT_OUTPUT) => {
                vega_runtime::InvalidBashAudit::from_json(&call.input_json).is_none()
                    && vega_tools::legacy_bash_input_is_invalid(&call.input_json)
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
            || call
                .output_text
                .as_deref()
                .is_some_and(vega_runtime::is_invalid_bash_feedback));
    let proposal = crate::types::ToolCall {
        id: call.id.clone(),
        tool: call.tool.clone(),
        input_json: call.input_json.clone(),
        codex_display: call
            .codex_display
            .as_ref()
            .and_then(CodexDisplayMetadata::from_stored),
    };
    let input = tool_card_input_projection(&proposal);
    if matches!(
        &input,
        ToolCardInputProjection::UserQuestion { input: Ok(_) }
    ) && matches!(
        status,
        ToolCallStatus::PendingApproval | ToolCallStatus::Approved | ToolCallStatus::Running
    ) {
        let result = crate::types::ToolResult {
            status: ToolCallStatus::Cancelled,
            output: crate::types::USER_QUESTION_CANCELLED.into(),
            codex_display: None,
            reused: true,
            exit_code: None,
            duration_ms: None,
            truncated: None,
            invalid: None,
        };
        return HistoryEntry::Tool {
            seq: call.seq,
            message_id: call.message_id.clone(),
            call_id: call.id.clone(),
            status: ToolCallStatus::Cancelled,
            approval,
            result: Some(tool_card_result_projection(Some(&input), &result)),
            input: Some(input),
        };
    }
    let invalid_superseded_claim = matches!(input, ToolCardInputProjection::ComputerUse { .. })
        && call.output_text.as_deref()
            == Some(vega_runtime::computer_use::SUPERSEDED_CONTROL_OUTPUT)
        && !approval_audit.as_ref().is_some_and(|audit| {
            audit.decision == Approval::Once
                && audit.source == ApprovalSource::ComputerUse
                && audit.note.is_none()
                && audit.danger.is_none()
        });
    let invalid_computer_use_audit = matches!(input, ToolCardInputProjection::ComputerUse { .. })
        && !approval_audit.as_ref().is_some_and(|audit| {
            let runtime_status = match status {
                ToolCallStatus::Success => vega_runtime::RuntimeToolStatus::Success,
                ToolCallStatus::Failed => vega_runtime::RuntimeToolStatus::Failed,
                ToolCallStatus::Rejected => vega_runtime::RuntimeToolStatus::Rejected,
                ToolCallStatus::Cancelled => vega_runtime::RuntimeToolStatus::Cancelled,
                _ => return false,
            };
            crate::agent::validate_recovered_projection(
                "",
                "",
                &call.id,
                &call.tool,
                &call.input_json,
                call.output_text.as_deref().unwrap_or_default(),
                runtime_status,
                audit,
                call.exit_code,
                call.duration_ms
                    .and_then(|duration| u64::try_from(duration).ok()),
            )
            .is_ok()
                && call.duration_ms.is_none()
        });
    let (input, result) = if invalid_bash_claim
        || invalid_computer_use_audit
        || invalid_superseded_claim
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
            codex_display: proposal.codex_display,
            reused: true,
            exit_code: call.exit_code,
            duration_ms: call.duration_ms.and_then(|ms| u64::try_from(ms).ok()),
            truncated: (matches!(
                input,
                ToolCardInputProjection::CodexAcp { .. }
                    | ToolCardInputProjection::ComputerUse { .. }
            ) && status == ToolCallStatus::Success)
                .then_some(false),
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

pub fn interrupted_tail(
    store: &Store,
    thread_id: &str,
) -> Result<Option<crate::types::InterruptedAssistant>, ConversationError> {
    messages::interrupted_tail(store.conn(), thread_id)
        .map(|tail| {
            tail.map(|(message_id, seq)| crate::types::InterruptedAssistant { message_id, seq })
        })
        .map_err(|error| ConversationError::Store(error.to_string()))
}

fn guard_legacy_history(store: &Store, thread: &str) -> Result<(), ConversationError> {
    if vega_store::thread_fork::has_archived_delegation(store.conn(), thread)
        .map_err(|error| ConversationError::Store(error.to_string()))?
    {
        return Err(ConversationError::UnsupportedHistoricalConsumer);
    }
    Ok(())
}

pub fn latest_historical_history_page(
    store: &Store,
    thread: &str,
    limit: usize,
) -> Result<crate::types::HistoricalHistoryPage, ConversationError> {
    assemble_historical(
        store,
        thread,
        page_read(store, thread, PageCursor::Head, limit)?,
        true,
    )
}

pub fn historical_history_page_before(
    store: &Store,
    thread: &str,
    cursor: PageCursor,
    limit: usize,
) -> Result<crate::types::HistoricalHistoryPage, ConversationError> {
    assemble_historical(
        store,
        thread,
        page_read(store, thread, cursor, limit)?,
        false,
    )
}

pub fn historical_history_page_after(
    store: &Store,
    thread: &str,
    cursor: i64,
    limit: usize,
) -> Result<crate::types::HistoricalHistoryPage, ConversationError> {
    let page = messages::page_after(store.conn(), thread, cursor, limit).map_err(page_failure)?;
    assemble_historical(store, thread, page, true)
}

pub fn historical_history_page_containing_message(
    store: &Store,
    thread: &str,
    message: &str,
    limit: usize,
) -> Result<Option<crate::types::HistoricalHistoryPage>, ConversationError> {
    messages::page_containing_message(store.conn(), thread, message, limit)
        .map_err(page_failure)?
        .map(|page| assemble_historical(store, thread, page, true))
        .transpose()
}

fn assemble_historical(
    store: &Store,
    thread: &str,
    mut page: MessagePage,
    attach_summary: bool,
) -> Result<crate::types::HistoricalHistoryPage, ConversationError> {
    use crate::types::{HistoricalHistoryEntry, HistoricalHistoryPage};
    let generated_notices = project_page_privacy(store, thread, &mut page)?;
    let projection = crate::privacy::DisplayProjection::load(store, thread)?;
    let archives = if let (Some(first), Some(last)) = (page.rows.first(), page.rows.last()) {
        vega_store::thread_fork::historical_receipts_in_range(
            store.conn(),
            thread,
            first.seq,
            last.seq,
        )
        .map_err(|error| ConversationError::Store(error.to_string()))?
    } else {
        Vec::new()
    };
    let mut delegations = HashMap::new();
    for call in page
        .tool_calls
        .iter()
        .filter(|call| vega_runtime::is_subagent_tool(&call.tool))
    {
        let archive = archives
            .iter()
            .find(|archive| archive.call_id == call.id && archive.kind == "delegation");
        let input = archive.map_or_else(
            || call.input_json.clone(),
            |archive| projection.json(&archive.input_json),
        );
        let receipt = archive.map_or_else(
            || {
                call.output_text
                    .as_deref()
                    .map(|text| project_historical_receipt(&projection, text))
            },
            |archive| {
                archive
                    .output_text
                    .as_ref()
                    .map(|text| project_historical_receipt(&projection, text))
            },
        );
        let mut references = archive.map_or_else(Vec::new, |archive| {
            archive
                .references
                .iter()
                .map(|reference| crate::types::HistoricalReference {
                    field: projection.text(&reference.field),
                    source_id: projection.text(&reference.source_id),
                    destination_id: reference
                        .destination_id
                        .as_ref()
                        .map(|id| projection.text(id)),
                })
                .collect()
        });
        if archive.is_none() {
            vega_store::thread_fork::delegation_references(
                &input,
                receipt.as_deref(),
                &mut references,
            )
            .map_err(|error| ConversationError::CorruptRow(error.to_string()))?;
        }
        let status = ToolCallStatus::parse(&call.status)
            .ok_or_else(|| ConversationError::CorruptRow("historical tool status".into()))?;
        delegations.insert(
            call.id.clone(),
            HistoricalHistoryEntry::Delegation {
                seq: call.seq,
                message_id: call.message_id.clone(),
                call_id: call.id.clone(),
                tool: call.tool.clone(),
                input_json: input,
                status,
                approval: call
                    .approval
                    .as_deref()
                    .and_then(|raw| ApprovalAudit::from_json(raw).ok())
                    .map(|audit| audit.decision),
                result: decode_historical_delegation(receipt, references)?,
            },
        );
    }
    let mut entries = project_rows_with(&page, &generated_notices, &|call| {
        if delegations.contains_key(&call.id) {
            HistoryEntry::Tool {
                seq: call.seq,
                message_id: call.message_id.clone(),
                call_id: call.id.clone(),
                status: ToolCallStatus::Success,
                approval: None,
                input: None,
                result: None,
            }
        } else {
            tool_entry(call)
        }
    })?;
    append_skill_history(store, thread, &page, &mut entries)?;
    append_task_summary(store, thread, &page, &mut entries, attach_summary)?;
    let entries = entries
        .into_iter()
        .map(|entry| {
            if let HistoryEntry::Tool { call_id, .. } = &entry
                && let Some(delegation) = delegations.remove(call_id)
            {
                return delegation;
            }
            HistoricalHistoryEntry::Ordinary(entry)
        })
        .collect();
    Ok(HistoricalHistoryPage {
        entries,
        older_cursor: page.older_cursor,
        newer_cursor: page.newer_cursor,
        newest_seq: page.rows.last().map(|row| row.seq),
    })
}

fn project_historical_receipt(
    projection: &crate::privacy::DisplayProjection,
    text: &str,
) -> String {
    let projected = projection.json(text);
    match (
        serde_json::from_str::<serde_json::Value>(text),
        serde_json::from_str::<serde_json::Value>(&projected),
    ) {
        (Ok(original), Ok(safe)) if original == safe => text.into(),
        _ => projected,
    }
}

fn decode_historical_delegation(
    receipt: Option<String>,
    references: Vec<crate::types::HistoricalReference>,
) -> Result<crate::types::HistoricalDelegationResult, ConversationError> {
    use crate::types::{HistoricalAgentSummary, HistoricalDelegationResult};
    let mut agents = Vec::new();
    if let Some(text) = &receipt
        && let Ok(value) = serde_json::from_str::<serde_json::Value>(text)
    {
        let recorded = value.get("agent").into_iter().chain(
            value
                .get("agents")
                .and_then(serde_json::Value::as_array)
                .into_iter()
                .flatten(),
        );
        for agent in recorded {
            let text = |field| {
                agent
                    .get(field)
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
            };
            agents.push(HistoricalAgentSummary {
                historical_agent_id: text("agent_id"),
                historical_message_id: text("message_id"),
                name: text("name"),
                state: text("state"),
                result: text("result"),
                release_state: text("release_state"),
                checkpoint_json: agent
                    .get("checkpoint")
                    .filter(|value| !value.is_null())
                    .map(serde_json::to_string)
                    .transpose()
                    .map_err(|_| {
                        ConversationError::CorruptRow("historical checkpoint JSON".into())
                    })?,
            });
        }
    }
    Ok(HistoricalDelegationResult {
        receipt,
        agents,
        references,
    })
}

fn project_page_privacy(
    store: &Store,
    thread_id: &str,
    page: &mut MessagePage,
) -> Result<Vec<HistoryEntry>, ConversationError> {
    let projection = crate::privacy::DisplayProjection::load(store, thread_id)?;
    let mut generated_notices = Vec::new();
    for row in &mut page.rows {
        for stored in page
            .generated_notices
            .iter()
            .filter(|notice| notice.message_id == row.id)
        {
            let notice = crate::types::GeneratedNoticeMetadata::from_store(stored)
                .filter(|notice| row.role == "assistant" && notice.is_valid_for(&row.content))
                .ok_or_else(|| {
                    ConversationError::CorruptRow("generated notice provenance".into())
                })?;
            let (display_content, positions) = projection.offsets(
                &row.content,
                &[notice.text_start_bytes, notice.text_end_bytes],
            );
            if let [start, end] = positions.as_slice()
                && display_content.get(*start..*end) == Some(notice.canonical_text().as_str())
            {
                generated_notices.push(HistoryEntry::GeneratedNotice {
                    seq: row.seq,
                    message_id: row.id.clone(),
                    notice,
                    display_text_start_bytes: *start,
                    display_text_end_bytes: *end,
                });
            }
        }
        let positions: Vec<_> = page
            .tool_calls
            .iter()
            .enumerate()
            .filter_map(|(index, call)| {
                (call.message_id == row.id)
                    .then_some((index, call.text_offset_bytes))
                    .and_then(|(index, offset)| {
                        offset
                            .and_then(|offset| usize::try_from(offset).ok())
                            .map(|offset| (index, offset))
                    })
            })
            .collect();
        let offsets: Vec<_> = positions.iter().map(|(_, offset)| *offset).collect();
        for response in page
            .assistant_responses
            .iter_mut()
            .filter(|response| response.message_id == row.id)
        {
            let (_, positions) = projection.offsets(
                &row.content,
                &[response.text_start_bytes, response.text_end_bytes],
            );
            if let [start, end] = positions.as_slice() {
                response.text_start_bytes = *start;
                response.text_end_bytes = *end;
            }
        }
        let (content, offsets) = projection.offsets(&row.content, &offsets);
        row.content = content;
        for ((index, _), offset) in positions.into_iter().zip(offsets) {
            page.tool_calls[index].text_offset_bytes = i64::try_from(offset).ok();
        }
        if let Some(note) = &mut row.plan_review_note {
            *note = projection.text(note);
        }
    }
    for call in &mut page.tool_calls {
        call.input_json = projection.json(&call.input_json);
        if let Some(output) = &mut call.output_text {
            *output = projection.text(output);
        }
    }
    Ok(generated_notices)
}
