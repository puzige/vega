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
    let thread = crate::threads::open_thread(store, thread_id)?;
    if thread.mode != crate::types::ThreadMode::Execute {
        return Ok(None);
    }
    let recent = store_messages::recent(store.conn(), thread_id, 1).map_err(store_error)?;
    let Some(instruction) = recent.first() else {
        return Ok(None);
    };
    if instruction.role != "user"
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
    Ok(Some(instruction.id.clone()))
}

/// Applies one review transaction. The returned instruction id is present
/// only for an approval winner and may be used to start the post-commit turn.
pub fn review_plan(
    store: &Store,
    thread_id: &str,
    plan_id: &str,
    action: PlanReviewAction,
) -> Result<PlanReviewOutcome, ConversationError> {
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
    let instruction = instruction_id
        .as_deref()
        .map(|id| store_messages::PlanInstruction {
            id,
            content: APPROVAL_INSTRUCTION,
            created_at: now,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::threads::{create_thread, set_thread_mode};
    use crate::types::{PermissionMode, ThreadMode};
    use vega_store::messages::{MessageRow, complete_plan, insert};

    fn setup() -> (Store, tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("vega.db")).unwrap();
        store.migrate().unwrap();
        store
            .conn()
            .execute(
                "INSERT INTO projects (id, path, name, created_at, last_opened_at) \
                 VALUES ('p', '/tmp/p', 'p', 0, 0)",
                [],
            )
            .unwrap();
        let thread = create_thread(&store, "p", "mock", PermissionMode::Confirm.as_str()).unwrap();
        set_thread_mode(&store, &thread.id, ThreadMode::Plan).unwrap();
        (store, dir, thread.id)
    }

    fn streaming_plan(store: &Store, thread_id: &str, id: &str, seq: i64) {
        insert(
            store.conn(),
            &MessageRow {
                id: id.into(),
                thread_id: thread_id.into(),
                seq,
                role: "assistant".into(),
                kind: "text".into(),
                content: String::new(),
                status: "streaming".into(),
                created_at: seq,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .unwrap();
    }

    #[test]
    fn newest_completion_supersedes_and_restart_loads_exact_state() {
        let (store, dir, thread_id) = setup();
        streaming_plan(&store, &thread_id, "a", 1);
        complete_plan(store.conn(), &thread_id, "a", "first", 10).unwrap();
        streaming_plan(&store, &thread_id, "b", 2);
        complete_plan(store.conn(), &thread_id, "b", "second", 20).unwrap();
        drop(store);
        let reopened = Store::open(dir.path().join("vega.db")).unwrap();
        reopened.migrate().unwrap();
        let plans = list_plans(&reopened, &thread_id).unwrap();
        assert_eq!(plans.len(), 2);
        assert_eq!(plans[0].status, PlanStatus::Abandoned);
        assert_eq!(plans[0].review_note.as_deref(), Some("superseded"));
        assert_eq!(plans[0].reviewed_at, Some(20));
        assert_eq!(plans[1].status, PlanStatus::Pending);
    }

    #[test]
    fn approval_is_single_winner_and_persists_execute_plus_instruction() {
        let (store, _dir, thread_id) = setup();
        streaming_plan(&store, &thread_id, "plan", 1);
        complete_plan(store.conn(), &thread_id, "plan", "steps", 10).unwrap();
        let first = review_plan(&store, &thread_id, "plan", PlanReviewAction::Approve).unwrap();
        let instruction_id = match first {
            PlanReviewOutcome::Applied {
                instruction_message_id: Some(id),
            } => id,
            _ => panic!("approval must return an instruction"),
        };
        assert_eq!(
            recoverable_approved_instruction(&store, &thread_id).unwrap(),
            Some(instruction_id.clone())
        );
        assert_eq!(
            review_plan(&store, &thread_id, "plan", PlanReviewAction::Approve).unwrap(),
            PlanReviewOutcome::Stale
        );
        let thread = vega_store::threads::find(store.conn(), &thread_id)
            .unwrap()
            .unwrap();
        assert_eq!(thread.mode, "execute");
        let user_count: i64 = store
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE thread_id=?1 AND role='user'",
                [&thread_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(user_count, 1);
        let next = vega_store::messages::next_seq(store.conn(), &thread_id).unwrap();
        vega_store::messages::insert(
            store.conn(),
            &MessageRow {
                id: "claimed-assistant".into(),
                thread_id: thread_id.clone(),
                seq: next,
                role: "assistant".into(),
                kind: "text".into(),
                content: String::new(),
                status: "streaming".into(),
                created_at: 20,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .unwrap();
        assert_eq!(
            recoverable_approved_instruction(&store, &thread_id).unwrap(),
            None
        );
    }

    #[test]
    fn change_and_abandon_do_not_switch_mode_or_insert_messages() {
        for action in [
            PlanReviewAction::RequestChanges {
                note: Some("revise".into()),
            },
            PlanReviewAction::Abandon { note: None },
        ] {
            let (store, _dir, thread_id) = setup();
            streaming_plan(&store, &thread_id, "plan", 1);
            complete_plan(store.conn(), &thread_id, "plan", "steps", 10).unwrap();
            assert!(matches!(
                review_plan(&store, &thread_id, "plan", action).unwrap(),
                PlanReviewOutcome::Applied {
                    instruction_message_id: None
                }
            ));
            let thread = vega_store::threads::find(store.conn(), &thread_id)
                .unwrap()
                .unwrap();
            assert_eq!(thread.mode, "plan");
        }
    }
}
