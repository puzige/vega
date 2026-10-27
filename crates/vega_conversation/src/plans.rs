//! Typed Plan loading and first-wins review orchestration.

use vega_store::Store;
use vega_store::messages as store_messages;

use crate::types::{ConversationError, Plan, PlanReviewAction, PlanReviewOutcome, PlanStatus};

const MAX_REVIEW_NOTE_BYTES: usize = 4 * 1024;
pub(crate) const APPROVAL_INSTRUCTION: &str =
    "The plan was approved. Execute the approved plan from conversation history.";

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or_default()
}

fn store_error<E: std::fmt::Display>(error: E) -> ConversationError {
    ConversationError::Store(error.to_string())
}

/// Loads validated plans in thread sequence order. Store validation rejects
/// invalid shapes and multiple pending plans before this mapping runs.
pub fn list_plans(store: &Store, thread_id: &str) -> Result<Vec<Plan>, ConversationError> {
    store_messages::plans_for_thread(store.conn(), thread_id)
        .map_err(store_error)?
        .into_iter()
        .map(|row| {
            let raw = row.plan_status.as_deref().ok_or_else(|| {
                ConversationError::CorruptRow("completed plan lacks status".to_string())
            })?;
            let status = PlanStatus::parse(raw).ok_or_else(|| {
                ConversationError::CorruptRow("plan status is outside vocabulary".to_string())
            })?;
            Ok(Plan {
                id: row.id,
                thread_id: row.thread_id,
                content: row.content,
                status,
                review_note: row.plan_review_note,
                reviewed_at: row.plan_reviewed_at,
            })
        })
        .collect()
}

/// Returns the exact latest unconsumed approval instruction, if one exists.
/// This is the restart-safe controller recovery seam for failures that occur
/// before the approved runner inserts its assistant claim.
pub fn recoverable_approved_instruction(
    store: &Store,
    thread_id: &str,
) -> Result<Option<String>, ConversationError> {
    let _coordination = crate::permission_contracts::coordinate()?;
    crate::permission_contracts::require_admission_open(store)?;
    Ok(recoverable_authority(store, thread_id)?.map(|(id, _)| id))
}

pub(crate) fn approved_authority(
    store: &Store,
    thread_id: &str,
    id: &str,
) -> Result<crate::types::PermissionFence, ConversationError> {
    recoverable_authority(store, thread_id)?
        .filter(|(saved, _)| saved == id)
        .map(|(_, fence)| fence)
        .ok_or(ConversationError::InvalidTaskIdentity)
}

fn recoverable_authority(
    store: &Store,
    thread_id: &str,
) -> Result<Option<(String, crate::types::PermissionFence)>, ConversationError> {
    let thread = crate::threads::get_thread(store, thread_id)?;
    if thread.mode != crate::types::ThreadMode::Execute {
        return Ok(None);
    }
    let recent = store_messages::recent(store.conn(), thread_id, 1).map_err(store_error)?;
    let Some(instruction) = recent.first() else {
        return Ok(None);
    };
    if !vega_store::permission_reset::replay_allowed(store.conn(), thread_id, instruction.seq)
        .map_err(store_error)?
        || instruction.role != "user"
        || instruction.kind != "text"
        || instruction.status != "done"
        || instruction.content != APPROVAL_INSTRUCTION
    {
        return Ok(None);
    }
    let next_seq = store_messages::next_seq(store.conn(), thread_id).map_err(store_error)?;
    if next_seq != instruction.seq + 1 {
        return Ok(None);
    }
    let matching = store_messages::plans_for_thread(store.conn(), thread_id)
        .map_err(store_error)?
        .into_iter()
        .filter(|plan| plan.seq < instruction.seq)
        .filter(|plan| {
            plan.plan_status.as_deref() == Some("approved")
                && plan.plan_reviewed_at == Some(instruction.created_at)
        })
        .count();
    if matching != 1 {
        return Err(ConversationError::CorruptRow(
            "approval instruction has invalid plan binding".into(),
        ));
    }
    let mut query = store.conn().prepare("SELECT permission_fence FROM plan_instruction_authority WHERE message_id=?1 AND thread_id=?2").map_err(store_error)?;
    let mut rows = query
        .query([instruction.id.as_str(), thread_id])
        .map_err(store_error)?;
    let Some(row) = rows.next().map_err(store_error)? else {
        return Ok(None);
    };
    let encoded: String = row.get(0).map_err(store_error)?;
    let saved: crate::types::PermissionFence =
        serde_json::from_str(&encoded).map_err(|_| ConversationError::InvalidTaskIdentity)?;
    let current = vega_store::threads::find(store.conn(), thread_id)
        .map_err(store_error)?
        .ok_or(ConversationError::InvalidTaskIdentity)?;
    if current.backend != "native"
        || current.status != "active"
        || !saved.is_valid()
        || saved != crate::permission_contracts::capture_fence(store, &current)?
    {
        return Ok(None);
    }
    Ok(Some((instruction.id.clone(), saved)))
}

/// Applies one review transaction. The returned instruction id is present
/// only for an approval winner and may be used to start the post-commit turn.
pub fn review_plan(
    store: &Store,
    thread_id: &str,
    plan_id: &str,
    action: PlanReviewAction,
) -> Result<PlanReviewOutcome, ConversationError> {
    let _coordination = crate::permission_contracts::coordinate()?;
    crate::permission_contracts::require_admission_open(store)?;
    let _admission = crate::thread_access::acquire(store, thread_id)?;
    let (status, note, instruction_id) = match action {
        PlanReviewAction::Approve => (
            PlanStatus::Approved,
            None,
            Some(ulid::Ulid::generate().to_string()),
        ),
        PlanReviewAction::RequestChanges { note } => {
            validate_note(note.as_deref())?;
            (PlanStatus::ChangesRequested, note, None)
        }
        PlanReviewAction::Abandon { note } => {
            validate_note(note.as_deref())?;
            (PlanStatus::Abandoned, note, None)
        }
    };
    let now = now_ms();
    let permission = instruction_id
        .as_ref()
        .map(|_| {
            let row = vega_store::threads::find(store.conn(), thread_id)
                .map_err(store_error)?
                .ok_or(ConversationError::InvalidTaskIdentity)?;
            let mut fence = crate::permission_contracts::capture_fence(store, &row)?;
            fence.run_mode = "execute".into();
            serde_json::to_string(&fence).map_err(store_error)
        })
        .transpose()?;
    let instruction =
        instruction_id
            .as_deref()
            .zip(permission.as_deref())
            .map(|(id, permission_fence)| store_messages::PlanInstruction {
                id,
                content: APPROVAL_INSTRUCTION,
                created_at: now,
                permission_fence,
            });
    let result = store_messages::review_plan(
        store.conn(),
        store_messages::PlanReview {
            thread_id,
            plan_id,
            status: status.as_str(),
            note: note.as_deref(),
            reviewed_at: now,
            instruction,
        },
    )
    .map_err(store_error)?;
    Ok(match result {
        store_messages::PlanReviewResult::Applied => PlanReviewOutcome::Applied {
            instruction_message_id: instruction_id,
        },
        store_messages::PlanReviewResult::Stale => PlanReviewOutcome::Stale,
    })
}

fn validate_note(note: Option<&str>) -> Result<(), ConversationError> {
    if note.is_some_and(|note| note.len() > MAX_REVIEW_NOTE_BYTES) {
        return Err(ConversationError::CorruptRow(
            "plan review note exceeds limit".to_string(),
        ));
    }
    Ok(())
}
