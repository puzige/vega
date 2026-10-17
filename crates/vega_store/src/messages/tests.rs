use std::sync::{Arc, Barrier};

use super::*;
use crate::Store;

fn setup() -> (Store, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("vega.db")).unwrap();
    store.migrate().unwrap();
    store
        .conn()
        .execute_batch(
            "INSERT INTO projects VALUES ('p','/tmp/p','p',NULL,0,0); \
             INSERT INTO threads (id,project_id,mode,model,created_at,updated_at) \
             VALUES ('t','p','plan','mock',0,0);",
        )
        .unwrap();
    (store, dir)
}

fn streaming(id: &str, seq: i64) -> MessageRow {
    MessageRow {
        id: id.into(),
        thread_id: "t".into(),
        seq,
        role: "assistant".into(),
        kind: "text".into(),
        content: String::new(),
        status: "streaming".into(),
        created_at: seq,
        plan_status: None,
        plan_review_note: None,
        plan_reviewed_at: None,
    }
}

#[test]
fn completion_promotes_text_and_supersedes_exact_old_pending() {
    let (store, _dir) = setup();
    insert(store.conn(), &streaming("one", 1)).unwrap();
    complete_plan(store.conn(), "t", "one", "first", 10, None).unwrap();
    insert(store.conn(), &streaming("two", 2)).unwrap();
    complete_plan(store.conn(), "t", "two", "second", 20, None).unwrap();
    let plans = plans_for_thread(store.conn(), "t").unwrap();
    assert_eq!(plans.len(), 2);
    assert_eq!(plans[0].plan_status.as_deref(), Some("abandoned"));
    assert_eq!(plans[0].plan_review_note.as_deref(), Some("superseded"));
    assert_eq!(plans[0].plan_reviewed_at, Some(20));
    assert_eq!(plans[1].kind, "plan");
    assert_eq!(plans[1].plan_status.as_deref(), Some("pending"));
}

#[test]
fn failed_current_promotion_rolls_back_supersede() {
    let (store, _dir) = setup();
    insert(store.conn(), &streaming("old", 1)).unwrap();
    complete_plan(store.conn(), "t", "old", "first", 10, None).unwrap();
    let error = complete_plan(store.conn(), "t", "missing", "second", 20, None).unwrap_err();
    assert!(matches!(error, PlanTransitionError::CorruptState));
    let old = find(store.conn(), "old").unwrap().unwrap();
    assert_eq!(old.plan_status.as_deref(), Some("pending"));
    assert_eq!(old.plan_reviewed_at, None);
}

#[test]
fn corrupt_metadata_fails_every_read_and_blocks_completion() {
    let (store, _dir) = setup();
    store
        .conn()
        .execute_batch(
            "INSERT INTO messages (id,thread_id,seq,role,kind,content,status,created_at,plan_status,plan_review_note) \
             VALUES ('bad','t',1,'assistant','plan','secret','done',0,'pending','illegal');",
        )
        .unwrap();
    insert(store.conn(), &streaming("new", 2)).unwrap();
    assert!(find(store.conn(), "bad").is_err());
    assert!(recent(store.conn(), "t", 10).is_err());
    assert!(plans_for_thread(store.conn(), "t").is_err());
    assert!(complete_plan(store.conn(), "t", "new", "new", 2, None).is_err());
    let new = find(store.conn(), "new").unwrap().unwrap();
    assert_eq!(new.status, "streaming");
}

#[test]
fn non_plan_metadata_is_not_hidden_from_plan_validation() {
    let (store, _dir) = setup();
    store
        .conn()
        .execute(
            "INSERT INTO messages (id,thread_id,seq,role,kind,content,status,created_at,plan_status) \
             VALUES ('bad','t',1,'assistant','text','x','done',0,'pending')",
            [],
        )
        .unwrap();
    assert!(plans_for_thread(store.conn(), "t").is_err());
}

#[test]
fn approved_plan_with_review_note_fails_all_reads() {
    let (store, _dir) = setup();
    store
        .conn()
        .execute(
            "INSERT INTO messages \
             (id,thread_id,seq,role,kind,content,status,created_at,plan_status,plan_review_note,plan_reviewed_at) \
             VALUES ('bad','t',1,'assistant','plan','steps','done',0,'approved','secret',1)",
            [],
        )
        .unwrap();
    assert!(find(store.conn(), "bad").is_err());
    assert!(recent(store.conn(), "t", 10).is_err());
    assert!(plans_for_thread(store.conn(), "t").is_err());
}

#[test]
fn approved_transition_with_note_is_rejected_without_mutation() {
    let (store, _dir) = setup();
    insert(store.conn(), &streaming("plan", 1)).unwrap();
    complete_plan(store.conn(), "t", "plan", "steps", 1, None).unwrap();
    let result = review_plan(
        store.conn(),
        PlanReview {
            thread_id: "t",
            plan_id: "plan",
            status: "approved",
            note: Some("illegal"),
            reviewed_at: 2,
            instruction: Some(PlanInstruction {
                id: "instruction",
                content: "approved",
                created_at: 2,
            }),
        },
    );
    assert!(matches!(result, Err(PlanTransitionError::CorruptState)));
    let plan = find(store.conn(), "plan").unwrap().unwrap();
    assert_eq!(plan.plan_status.as_deref(), Some("pending"));
    assert!(find(store.conn(), "instruction").unwrap().is_none());
}

#[test]
fn obsolete_streaming_plan_shape_is_corrupt_everywhere() {
    let (store, _dir) = setup();
    store
        .conn()
        .execute(
            "INSERT INTO messages (id,thread_id,seq,role,kind,content,status,created_at) \
             VALUES ('bad','t',1,'assistant','plan','partial','streaming',0)",
            [],
        )
        .unwrap();
    insert(store.conn(), &streaming("new", 2)).unwrap();
    assert!(find(store.conn(), "bad").is_err());
    assert!(plans_for_thread(store.conn(), "t").is_err());
    assert!(complete_plan(store.conn(), "t", "new", "new", 3, None).is_err());
}

#[test]
fn review_distinguishes_terminal_stale_from_corrupt_pending() {
    let (store, _dir) = setup();
    insert(store.conn(), &streaming("plan", 1)).unwrap();
    complete_plan(store.conn(), "t", "plan", "steps", 10, None).unwrap();
    let applied = review_plan(
        store.conn(),
        PlanReview {
            thread_id: "t",
            plan_id: "plan",
            status: "abandoned",
            note: None,
            reviewed_at: 11,
            instruction: None,
        },
    )
    .unwrap();
    assert_eq!(applied, PlanReviewResult::Applied);
    let stale = review_plan(
        store.conn(),
        PlanReview {
            thread_id: "t",
            plan_id: "plan",
            status: "abandoned",
            note: None,
            reviewed_at: 12,
            instruction: None,
        },
    )
    .unwrap();
    assert_eq!(stale, PlanReviewResult::Stale);

    store
        .conn()
        .execute_batch(
            "UPDATE messages SET plan_status='pending',plan_review_note='bad',plan_reviewed_at=NULL WHERE id='plan';",
        )
        .unwrap();
    assert!(
        review_plan(
            store.conn(),
            PlanReview {
                thread_id: "t",
                plan_id: "plan",
                status: "abandoned",
                note: None,
                reviewed_at: 13,
                instruction: None,
            }
        )
        .is_err()
    );
}

#[test]
fn separate_connections_serialize_review_to_one_winner() {
    let (store, dir) = setup();
    insert(store.conn(), &streaming("plan", 1)).unwrap();
    complete_plan(store.conn(), "t", "plan", "steps", 10, None).unwrap();
    drop(store);
    let barrier = Arc::new(Barrier::new(3));
    let mut workers = Vec::new();
    for status in ["approved", "abandoned"] {
        let path = dir.path().join("vega.db");
        let barrier = barrier.clone();
        workers.push(std::thread::spawn(move || {
            let connection = Connection::open(path).unwrap();
            connection
                .busy_timeout(std::time::Duration::from_secs(5))
                .unwrap();
            barrier.wait();
            let instruction = (status == "approved").then_some(PlanInstruction {
                id: "approval-instruction",
                content: "approved",
                created_at: 20,
            });
            review_plan(
                &connection,
                PlanReview {
                    thread_id: "t",
                    plan_id: "plan",
                    status,
                    note: None,
                    reviewed_at: 20,
                    instruction,
                },
            )
            .unwrap()
        }));
    }
    barrier.wait();
    let results: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    assert_eq!(
        results
            .iter()
            .filter(|result| **result == PlanReviewResult::Applied)
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|result| **result == PlanReviewResult::Stale)
            .count(),
        1
    );
}

#[test]
fn completion_and_old_approval_obey_both_commit_orders() {
    let (store, _dir) = setup();
    insert(store.conn(), &streaming("old", 1)).unwrap();
    complete_plan(store.conn(), "t", "old", "old", 10, None).unwrap();
    insert(store.conn(), &streaming("new", 2)).unwrap();
    complete_plan(store.conn(), "t", "new", "new", 20, None).unwrap();
    let stale = review_plan(
        store.conn(),
        PlanReview {
            thread_id: "t",
            plan_id: "old",
            status: "approved",
            note: None,
            reviewed_at: 21,
            instruction: Some(PlanInstruction {
                id: "late",
                content: "late",
                created_at: 21,
            }),
        },
    )
    .unwrap();
    assert_eq!(stale, PlanReviewResult::Stale);

    let (store, _dir) = setup();
    insert(store.conn(), &streaming("old", 1)).unwrap();
    complete_plan(store.conn(), "t", "old", "old", 10, None).unwrap();
    insert(store.conn(), &streaming("new", 2)).unwrap();
    assert_eq!(
        review_plan(
            store.conn(),
            PlanReview {
                thread_id: "t",
                plan_id: "old",
                status: "approved",
                note: None,
                reviewed_at: 20,
                instruction: Some(PlanInstruction {
                    id: "winner",
                    content: "winner",
                    created_at: 20,
                }),
            },
        )
        .unwrap(),
        PlanReviewResult::Applied
    );
    assert!(complete_plan(store.conn(), "t", "new", "new", 21, None).is_err());
    let current = find(store.conn(), "new").unwrap().unwrap();
    assert_eq!(current.status, "streaming");
    assert_eq!(current.kind, "text");
}

#[test]
fn separate_connections_serialize_completions_to_one_pending() {
    let (store, dir) = setup();
    insert(store.conn(), &streaming("a", 1)).unwrap();
    insert(store.conn(), &streaming("b", 2)).unwrap();
    drop(store);
    let barrier = Arc::new(Barrier::new(3));
    let mut workers = Vec::new();
    for (id, now) in [("a", 10), ("b", 20)] {
        let path = dir.path().join("vega.db");
        let barrier = barrier.clone();
        workers.push(std::thread::spawn(move || {
            let connection = Connection::open(path).unwrap();
            connection
                .busy_timeout(std::time::Duration::from_secs(5))
                .unwrap();
            barrier.wait();
            complete_plan(&connection, "t", id, id, now, None).unwrap();
        }));
    }
    barrier.wait();
    for worker in workers {
        worker.join().unwrap();
    }
    let reopened = Store::open(dir.path().join("vega.db")).unwrap();
    let plans = plans_for_thread(reopened.conn(), "t").unwrap();
    assert_eq!(plans.len(), 2);
    assert_eq!(
        plans
            .iter()
            .filter(|plan| plan.plan_status.as_deref() == Some("pending"))
            .count(),
        1
    );
    assert_eq!(
        plans
            .iter()
            .filter(|plan| plan.plan_status.as_deref() == Some("abandoned")
                && plan.plan_review_note.as_deref() == Some("superseded"))
            .count(),
        1
    );
}

#[test]
fn issue401_migration_backfills_only_adjacent_delivered_steer_segments() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("vega.db")).unwrap();
    for migration in crate::MIGRATIONS.iter().take(25) {
        store.conn().execute_batch(migration).unwrap();
    }
    store
        .conn()
        .pragma_update(None, "user_version", 25_u32)
        .unwrap();
    store
        .conn()
        .execute_batch(
            "INSERT INTO projects VALUES ('p','/tmp/p','p',NULL,0,0); \
             INSERT INTO threads (id,project_id,mode,model,created_at,updated_at) \
             VALUES ('t','p','plan','mock',0,0); \
             INSERT INTO messages (id,thread_id,seq,role,kind,content,status,created_at) VALUES \
             ('original-user','t',1,'user','text','start','done',1), \
             ('root','t',2,'assistant','text','first','done',2), \
             ('steer-user-one','t',3,'user','text','one','done',3), \
             ('middle','t',4,'assistant','text','second','done',4), \
             ('steer-user-two','t',5,'user','text','two','done',5), \
             ('terminal','t',6,'assistant','text','third','done',6), \
             ('queue-user','t',7,'user','text','queued','done',7), \
             ('ordinary','t',8,'assistant','text','separate','done',8); \
             INSERT INTO pending_messages \
             (id,thread_id,ordinal,behavior,generation,payload,state,created_at) VALUES \
             ('steer-user-one','t',0,'steer','generation','', 'delivered',3), \
             ('steer-user-two','t',1,'steer','generation','', 'delivered',5), \
             ('queue-user','t',2,'queue','generation','', 'delivered',7);",
        )
        .unwrap();
    store.migrate().unwrap();
    assert_eq!(
        assistant_run_activity_id(store.conn(), "root")
            .unwrap()
            .as_deref(),
        Some("root")
    );
    assert_eq!(
        assistant_run_activity_id(store.conn(), "middle")
            .unwrap()
            .as_deref(),
        Some("root")
    );
    assert_eq!(
        assistant_run_activity_id(store.conn(), "terminal")
            .unwrap()
            .as_deref(),
        Some("root")
    );
    assert_eq!(
        assistant_run_activity_id(store.conn(), "ordinary").unwrap(),
        None
    );
}

#[test]
fn issue401_assistant_run_activity_projection_keeps_one_terminal_owner() {
    let (store, _dir) = setup();
    for (id, seq, duration) in [
        ("root", 2, None),
        ("middle", 4, None),
        ("terminal", 6, Some(12_345)),
    ] {
        insert(store.conn(), &streaming(id, seq)).unwrap();
        finish_streaming(store.conn(), id, "answer", "done", duration).unwrap();
    }
    associate_assistant_run_activity(store.conn(), "root", "root").unwrap();
    associate_assistant_run_activity(store.conn(), "middle", "root").unwrap();
    associate_assistant_run_activity(store.conn(), "terminal", "root").unwrap();
    let page = page_before(store.conn(), "t", PageCursor::Head, 20).unwrap();
    assert_eq!(page.assistant_run_activities.len(), 3);
    assert!(page.assistant_run_activities.iter().all(|activity| {
        activity.run_id == "root"
            && activity.terminal_message_id.as_deref() == Some("terminal")
            && activity.terminal_status.as_deref() == Some("done")
            && activity.execution_duration_ms == Some(12_345)
            && activity.member_count == 3
    }));
    assert_eq!(
        assistant_run_activity_id(store.conn(), "middle")
            .unwrap()
            .as_deref(),
        Some("root")
    );
    assert_eq!(
        assistant_run_activity_id(store.conn(), "unmapped").unwrap(),
        None
    );
}

#[test]
fn issue401_activity_association_is_idempotent_but_rejects_conflicts() {
    let (store, _dir) = setup();
    insert(store.conn(), &streaming("root", 2)).unwrap();
    insert(store.conn(), &streaming("next", 4)).unwrap();
    associate_assistant_run_activity(store.conn(), "root", "root").unwrap();
    associate_assistant_run_activity(store.conn(), "next", "root").unwrap();
    associate_assistant_run_activity(store.conn(), "next", "root").unwrap();
    assert!(associate_assistant_run_activity(store.conn(), "next", "next").is_err());
    assert_eq!(
        assistant_run_activity_id(store.conn(), "next")
            .unwrap()
            .as_deref(),
        Some("root")
    );
}

#[test]
fn issue401_activity_projection_uses_latest_message_for_a_live_run() {
    let (store, _dir) = setup();
    insert(store.conn(), &streaming("root", 2)).unwrap();
    finish_streaming(store.conn(), "root", "first", "done", None).unwrap();
    insert(store.conn(), &streaming("next", 4)).unwrap();
    associate_assistant_run_activity(store.conn(), "root", "root").unwrap();
    associate_assistant_run_activity(store.conn(), "next", "root").unwrap();
    let page = page_before(store.conn(), "t", PageCursor::Head, 20).unwrap();
    assert!(page.assistant_run_activities.iter().all(|activity| {
        activity.run_id == "root"
            && activity.terminal_message_id.as_deref() == Some("next")
            && activity.terminal_status.is_none()
            && activity.execution_duration_ms.is_none()
    }));
}

#[test]
fn issue401_membership_rejects_non_assistant_owners() {
    let (store, _dir) = setup();
    insert(store.conn(), &streaming("root", 2)).unwrap();
    assert!(associate_assistant_run_activity(store.conn(), "root", "missing").is_err());
    store
        .conn()
        .execute(
            "INSERT INTO messages(id,thread_id,seq,role,kind,content,status,created_at) \
         VALUES ('user','t',1,'user','text','hi','done',1)",
            [],
        )
        .unwrap();
    assert!(associate_assistant_run_activity(store.conn(), "user", "root").is_err());
}

#[test]
fn issue146_terminal_durations_are_persisted_and_projected_by_page() {
    let (store, _dir) = setup();
    for (id, status, duration) in [
        ("done", "done", 10_005),
        ("failed", "failed", 1_201),
        ("stopped", "interrupted", 15),
    ] {
        let seq = next_seq(store.conn(), "t").unwrap();
        insert(store.conn(), &streaming(id, seq)).unwrap();
        assert_eq!(
            finish_streaming(store.conn(), id, "answer", status, Some(duration)).unwrap(),
            1
        );
    }
    let seq = next_seq(store.conn(), "t").unwrap();
    insert(store.conn(), &streaming("legacy", seq)).unwrap();
    finish_streaming(store.conn(), "legacy", "old", "done", None).unwrap();
    let seq = next_seq(store.conn(), "t").unwrap();
    insert(store.conn(), &streaming("plan", seq)).unwrap();
    complete_plan(store.conn(), "t", "plan", "steps", 100, Some(62_001)).unwrap();

    let page = page_before(store.conn(), "t", PageCursor::Head, 20).unwrap();
    assert_eq!(page.execution_durations_ms.len(), 4);
    assert_eq!(page.execution_durations_ms.get("done"), Some(&10_005));
    assert_eq!(page.execution_durations_ms.get("failed"), Some(&1_201));
    assert_eq!(page.execution_durations_ms.get("stopped"), Some(&15));
    assert_eq!(page.execution_durations_ms.get("plan"), Some(&62_001));
    assert!(!page.execution_durations_ms.contains_key("legacy"));

    let seq = next_seq(store.conn(), "t").unwrap();
    insert(store.conn(), &streaming("invalid", seq)).unwrap();
    assert!(finish_streaming(store.conn(), "invalid", "partial", "streaming", Some(9)).is_err());
    assert_eq!(
        find(store.conn(), "invalid").unwrap().unwrap().status,
        "streaming"
    );
    assert!(finish_streaming(store.conn(), "invalid", "partial", "failed", Some(-1)).is_err());
    assert_eq!(
        find(store.conn(), "invalid").unwrap().unwrap().status,
        "streaming"
    );
}

// ─── keyset page contract (S8-T45/C7) ────────────────────────────────────

/// Seeds `count` terminal assistant rows plus one user row per message
/// (2 rows per step, seqs 1..=2*count) and returns the store.
fn seed_page_thread(count: usize) -> (Store, tempfile::TempDir) {
    let (store, dir) = setup();
    for step in 0..count {
        insert(
            store.conn(),
            &MessageRow {
                id: format!("user-{step}"),
                thread_id: "t".into(),
                seq: (2 * step + 1) as i64,
                role: "user".into(),
                kind: "text".into(),
                content: format!("问 {step}"),
                status: "done".into(),
                created_at: 1,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .unwrap();
        insert(
            store.conn(),
            &MessageRow {
                id: format!("assistant-{step}"),
                thread_id: "t".into(),
                seq: (2 * step + 2) as i64,
                role: "assistant".into(),
                kind: "text".into(),
                content: format!("答 {step}"),
                status: "done".into(),
                created_at: 1,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .unwrap();
    }
    (store, dir)
}

fn tool_call(id: &str, message_id: &str, seq: i64) -> String {
    format!(
        "INSERT INTO tool_calls \
         (id, thread_id, message_id, seq, tool, input_json, output_text, status, created_at) \
         VALUES ('{id}', 't', '{message_id}', {seq}, 'bash', '{{}}', 'out', 'success', 1)"
    )
}

#[test]
fn page_size_boundaries_reject_zero_and_one_over_the_cap() {
    let (store, _dir) = seed_page_thread(3);
    let conn = store.conn();
    for rejected in [0usize, PAGE_LIMIT + 1, 5000] {
        let error = page_before(conn, "t", PageCursor::Head, rejected).unwrap_err();
        assert!(
            matches!(error, PageRequestError::InvalidPageSize(size) if size == rejected),
            "page size {rejected} must be refused, got {error:?}"
        );
    }
    // 1 / 199 / 200 stay inside the contract (200 = full thread here).
    assert_eq!(
        page_before(conn, "t", PageCursor::Head, 1)
            .unwrap()
            .rows
            .len(),
        1
    );
    assert_eq!(
        page_before(conn, "t", PageCursor::Head, PAGE_LIMIT)
            .unwrap()
            .rows
            .len(),
        6
    );
}

#[test]
fn empty_thread_pages_to_exhaustion_without_cursor() {
    let (store, _dir) = setup();
    let page = page_before(store.conn(), "t", PageCursor::Head, PAGE_LIMIT).unwrap();
    assert!(page.rows.is_empty());
    assert_eq!(page.older_cursor, None);
    assert!(page.tool_calls.is_empty());
}

#[test]
fn exactly_one_page_has_no_older_cursor() {
    let (store, _dir) = seed_page_thread(100); // 200 rows exactly
    let page = page_before(store.conn(), "t", PageCursor::Head, PAGE_LIMIT).unwrap();
    assert_eq!(page.rows.len(), 200);
    // The page is full yet there is nothing older: one extra read proves
    // exhaustion (a full page alone must not imply more history).
    assert_eq!(page.older_cursor, Some(1));
    let older = page_before(
        store.conn(),
        "t",
        PageCursor::Before(page.older_cursor.unwrap()),
        PAGE_LIMIT,
    )
    .unwrap();
    assert!(older.rows.is_empty());
    assert_eq!(older.older_cursor, None);
}

#[test]
fn cursor_walk_covers_every_seq_exactly_once() {
    let (store, _dir) = seed_page_thread(250); // 500 rows
    let conn = store.conn();
    let mut seen: Vec<i64> = Vec::new();
    let mut cursor = PageCursor::Head;
    let mut rounds = 0;
    loop {
        let page = page_before(conn, "t", cursor, 199).unwrap();
        for row in &page.rows {
            seen.push(row.seq);
        }
        rounds += 1;
        match page.older_cursor {
            Some(seq) => cursor = PageCursor::Before(seq),
            None => break,
        }
    }
    assert_eq!(seen.len(), 500, "no duplicates and no gaps");
    // Pages arrive newest→oldest; sorted, the union must be exactly the
    // thread's seq range with nothing missing and nothing repeated.
    seen.sort_unstable();
    assert!(
        seen.iter().copied().eq(1..=500),
        "keyset walk must be strictly continuous"
    );
    assert_eq!(rounds, 3, "500 rows over 199-sized pages: 3 reads");
}

#[test]
fn message_id_page_is_thread_scoped_bounded_and_opens_newer_cursor() {
    let (store, _dir) = seed_page_thread(300);
    let page = page_containing_message(store.conn(), "t", "user-152", 100)
        .unwrap()
        .unwrap();
    let seqs: Vec<i64> = page.rows.iter().map(|row| row.seq).collect();
    assert_eq!(seqs.len(), 100);
    assert!(seqs.iter().copied().eq(256..=355));
    assert_eq!(page.rows[49].id, "user-152");
    assert_eq!(page.older_cursor, Some(256));
    assert_eq!(page.newer_cursor, Some(355));
    assert!(page.tool_calls.is_empty());
    assert!(page.images.is_empty());
}

#[test]
fn message_id_page_returns_none_for_unknown_or_foreign_id() {
    let (store, _dir) = seed_page_thread(10);
    store
        .conn()
        .execute_batch(
            "INSERT INTO threads (id,project_id,mode,model,created_at,updated_at) \
             VALUES ('other','p','plan','mock',0,0);",
        )
        .unwrap();
    insert(
        store.conn(),
        &MessageRow {
            id: "foreign-message".into(),
            thread_id: "other".into(),
            seq: 1,
            role: "user".into(),
            kind: "text".into(),
            content: "外部线程内容".into(),
            status: "done".into(),
            created_at: 1,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .unwrap();
    assert!(
        page_containing_message(store.conn(), "t", "missing", 100)
            .unwrap()
            .is_none()
    );
    assert!(
        page_containing_message(store.conn(), "t", "foreign-message", 100)
            .unwrap()
            .is_none()
    );
    assert!(
        page_containing_message(store.conn(), "other", "foreign-message", 100)
            .unwrap()
            .is_some()
    );
}

#[test]
fn newer_pages_continue_without_gaps_or_duplicates() {
    let (store, _dir) = seed_page_thread(300);
    let first = page_containing_message(store.conn(), "t", "user-152", 100)
        .unwrap()
        .unwrap();
    let older = page_before(
        store.conn(),
        "t",
        PageCursor::Before(first.older_cursor.unwrap()),
        100,
    )
    .unwrap();
    let second = page_after(store.conn(), "t", first.newer_cursor.unwrap(), 100).unwrap();
    let third = page_after(store.conn(), "t", second.newer_cursor.unwrap(), 100).unwrap();
    let fourth = page_after(store.conn(), "t", third.newer_cursor.unwrap(), 100).unwrap();
    let seqs: Vec<i64> = older.rows.iter().map(|row| row.seq).collect();
    assert!(seqs.iter().copied().eq(156..=255));
    let seqs: Vec<i64> = first
        .rows
        .iter()
        .chain(&second.rows)
        .chain(&third.rows)
        .chain(&fourth.rows)
        .map(|row| row.seq)
        .collect();
    assert!(seqs.iter().copied().eq(256..=600));
    assert_eq!(fourth.rows.len(), 45);
    assert_eq!(fourth.newer_cursor, None);
}

#[test]
fn page_after_rejects_sizes_outside_the_contract() {
    let (store, _dir) = seed_page_thread(3);
    for rejected in [0usize, PAGE_LIMIT + 1, 5000] {
        let error = page_after(store.conn(), "t", 0, rejected).unwrap_err();
        assert!(matches!(error, PageRequestError::InvalidPageSize(size) if size == rejected));
    }
}

#[test]
fn page_after_older_cursor_marks_the_durable_oldest_boundary() {
    let (store, _dir) = seed_page_thread(3);
    let first = page_after(store.conn(), "t", 0, 3).unwrap();
    assert!(first.rows.iter().map(|row| row.seq).eq(1..=3));
    assert_eq!(first.older_cursor, None);
    let second = page_after(store.conn(), "t", 3, 3).unwrap();
    assert!(second.rows.iter().map(|row| row.seq).eq(4..=6));
    assert_eq!(second.older_cursor, Some(4));
}

#[test]
fn interrupted_and_failed_rows_are_durable_streaming_is_not() {
    let (store, _dir) = setup();
    for (id, seq, status) in [
        ("done", 1, "done"),
        ("interrupted", 2, "interrupted"),
        ("failed", 3, "failed"),
        ("streaming", 4, "streaming"),
    ] {
        insert(
            store.conn(),
            &MessageRow {
                id: id.into(),
                thread_id: "t".into(),
                seq,
                role: "assistant".into(),
                kind: "text".into(),
                content: id.into(),
                status: status.into(),
                created_at: 1,
                plan_status: None,
                plan_review_note: None,
                plan_reviewed_at: None,
            },
        )
        .unwrap();
    }
    let page = page_before(store.conn(), "t", PageCursor::Head, PAGE_LIMIT).unwrap();
    let statuses: Vec<&str> = page
        .rows
        .iter()
        .map(|row| (row.id.as_str(), row.status.as_str()).0)
        .collect();
    assert_eq!(statuses, vec!["done", "interrupted", "failed"]);
}

#[test]
fn mid_run_insert_never_reuses_or_steals_a_cursor_page() {
    let (store, _dir) = seed_page_thread(250); // 500 rows
    let conn = store.conn();
    let head = page_before(conn, "t", PageCursor::Head, 200).unwrap();
    assert_eq!(head.rows.first().unwrap().seq, 301);
    assert_eq!(head.rows.last().unwrap().seq, 500);
    // New work lands strictly above every loaded seq (next_seq = MAX+1).
    insert(
        conn,
        &MessageRow {
            id: "late".into(),
            thread_id: "t".into(),
            seq: next_seq(conn, "t").unwrap(),
            role: "user".into(),
            kind: "text".into(),
            content: "mid-run".into(),
            status: "done".into(),
            created_at: 2,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .unwrap();
    let older = page_before(
        conn,
        "t",
        PageCursor::Before(head.older_cursor.unwrap()),
        200,
    )
    .unwrap();
    assert_eq!(older.rows.len(), 200);
    assert_eq!(older.rows.first().unwrap().seq, 101);
    assert_eq!(older.rows.last().unwrap().seq, 300);
    assert_eq!(older.older_cursor, Some(101));
}

#[test]
fn reopening_the_thread_resets_cursor_to_the_newest_page() {
    let (store, dir) = seed_page_thread(250);
    drop(store);
    let reopened = Store::open(dir.path().join("vega.db")).unwrap();
    let first = page_before(reopened.conn(), "t", PageCursor::Head, 200).unwrap();
    assert_eq!(first.rows.first().unwrap().seq, 301);
    assert_eq!(first.rows.last().unwrap().seq, 500);
    // A fresh open always starts from Head again; the old in-memory cursor
    // is view state, not durable state.
    let again = page_before(reopened.conn(), "t", PageCursor::Head, 200).unwrap();
    assert_eq!(again, first);
}

#[test]
fn page_batches_tool_calls_of_exactly_its_messages() {
    let (store, _dir) = seed_page_thread(4); // 8 rows
    let conn = store.conn();
    conn.execute_batch(&tool_call("tc-old-1", "assistant-0", 1))
        .unwrap();
    conn.execute_batch(&tool_call("tc-old-2", "assistant-0", 2))
        .unwrap();
    conn.execute_batch(&tool_call("tc-new", "assistant-3", 3))
        .unwrap();
    let head = page_before(conn, "t", PageCursor::Head, 4).unwrap();
    // Newest 4 rows = user-2, assistant-2, user-3, assistant-3: only the
    // newest tool call belongs to this page.
    let ids: Vec<&str> = head
        .tool_calls
        .iter()
        .map(|call| call.id.as_str())
        .collect();
    assert_eq!(ids, vec!["tc-new"]);
    assert_eq!(head.tool_calls[0].message_id, "assistant-3");
    let older = page_before(conn, "t", PageCursor::Before(head.older_cursor.unwrap()), 4).unwrap();
    let ids: Vec<&str> = older
        .tool_calls
        .iter()
        .map(|call| call.id.as_str())
        .collect();
    assert_eq!(ids, vec!["tc-old-1", "tc-old-2"], "ordered by call seq");
    assert_eq!(older.tool_calls[0].message_id, "assistant-0");
}

#[test]
fn page_read_issues_constant_statements_regardless_of_row_count() {
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

    static STATEMENTS: AtomicUsize = AtomicUsize::new(0);
    unsafe extern "C" fn trace_statements(
        code: u32,
        context: *mut std::ffi::c_void,
        _first: *mut std::ffi::c_void,
        _second: *mut std::ffi::c_void,
    ) -> i32 {
        // SQLITE_TRACE_STMT == 1: one compiled application statement.
        if code == 1 {
            let counter = unsafe { &*(context as *const AtomicUsize) };
            counter.fetch_add(1, AtomicOrdering::SeqCst);
        }
        0
    }
    let install_trace = |conn: &Connection| {
        let db = unsafe { conn.handle() };
        let registered = unsafe {
            rusqlite::ffi::sqlite3_trace_v2(
                db,
                1,
                Some(trace_statements),
                &STATEMENTS as *const AtomicUsize as *mut std::ffi::c_void,
            )
        };
        assert_eq!(registered, 0, "trace hook registration");
    };

    let (small, _small_dir) = seed_page_thread(1);
    let (large, _large_dir) = seed_page_thread(100);
    install_trace(small.conn());
    install_trace(large.conn());

    STATEMENTS.store(0, AtomicOrdering::SeqCst);
    page_before(small.conn(), "t", PageCursor::Head, PAGE_LIMIT).unwrap();
    let small_statements = STATEMENTS.load(AtomicOrdering::SeqCst);

    STATEMENTS.store(0, AtomicOrdering::SeqCst);
    page_before(large.conn(), "t", PageCursor::Head, PAGE_LIMIT).unwrap();
    let large_statements = STATEMENTS.load(AtomicOrdering::SeqCst);

    assert_eq!(
        small_statements, large_statements,
        "per-page statement count must not scale with rows (C7 零 N+1)"
    );
    // The run-activity membership projection adds one constant statement, so the
    // bound stays independent of the number of rows on the page.
    assert!(
        large_statements <= 9,
        "one page stays a bounded constant statement count, got {large_statements}"
    );
}

#[test]
fn page_and_tool_batch_read_on_one_fresh_snapshot() {
    // A second connection writes between the page read and its tool batch;
    // the page API must never surface the mixed state, so the newest page
    // cannot gain tool rows that its own snapshot did not contain.
    let (store, dir) = seed_page_thread(2); // 4 rows
    let writer = Connection::open(dir.path().join("vega.db")).unwrap();
    writer
        .busy_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    let page = page_before(store.conn(), "t", PageCursor::Head, PAGE_LIMIT).unwrap();
    assert!(page.tool_calls.is_empty());
    writer
        .execute_batch(&tool_call("tc-after", "assistant-1", 9))
        .unwrap();
    let page_again = page_before(store.conn(), "t", PageCursor::Head, PAGE_LIMIT).unwrap();
    assert_eq!(page_again.tool_calls.len(), 1, "new snapshot sees the row");
    let ids: Vec<&str> = page_again
        .tool_calls
        .iter()
        .map(|call| call.id.as_str())
        .collect();
    assert_eq!(ids, vec!["tc-after"]);
}

#[test]
fn issue296_response_metadata_replay_and_invalid_spans_preserve_body() {
    let (store, _dir) = setup();
    insert(store.conn(), &streaming("response", 1)).unwrap();
    let body = "过程。结果。";
    update_streaming_content(store.conn(), "response", body).unwrap();
    let mut response = AssistantResponseRow {
        message_id: "response".into(),
        response_id: "first".into(),
        text_start_bytes: 0,
        text_end_bytes: "过程。".len(),
        kind: "commentary".into(),
    };
    assert!(record_assistant_response(store.conn(), "response", body, &response).unwrap());
    assert!(record_assistant_response(store.conn(), "response", body, &response).unwrap());
    response.text_end_bytes = body.len();
    assert!(!record_assistant_response(store.conn(), "response", body, &response).unwrap());
    response.response_id = "invalid".into();
    for (start, end) in [(1, 9), (9, 10), (9, body.len() + 1), (0, body.len())] {
        response.text_start_bytes = start;
        response.text_end_bytes = end;
        assert!(!record_assistant_response(store.conn(), "response", body, &response).unwrap());
    }
    response.response_id = "final".into();
    response.text_start_bytes = "过程。".len();
    response.text_end_bytes = body.len();
    response.kind = "final".into();
    assert!(record_assistant_response(store.conn(), "response", body, &response).unwrap());
    finish_streaming(store.conn(), "response", body, "done", Some(1024)).unwrap();
    let page = page_before(store.conn(), "t", PageCursor::Head, 20).unwrap();
    assert_eq!(page.rows[0].content, body);
    assert_eq!(page.assistant_responses.len(), 2);
    assert_eq!(page.assistant_responses[1], response);
    store
        .conn()
        .execute(
            "UPDATE assistant_responses SET text_start_bytes = 10 WHERE response_id = 'final'",
            [],
        )
        .unwrap();
    let page = page_before(store.conn(), "t", PageCursor::Head, 20).unwrap();
    assert!(page.assistant_responses.is_empty());
    assert_eq!(page.rows[0].content, body);
    assert_eq!(page.execution_durations_ms.get("response"), Some(&1024));
}

#[test]
fn issue296_response_metadata_failed_and_legacy_runs_never_claim_final() {
    let (store, _dir) = setup();
    for (seq, id) in [(1, "failed"), (2, "legacy")] {
        insert(store.conn(), &streaming(id, seq)).unwrap();
        update_streaming_content(store.conn(), id, "result").unwrap();
    }
    assert!(
        record_assistant_response(
            store.conn(),
            "failed",
            "result",
            &AssistantResponseRow {
                message_id: "failed".into(),
                response_id: "final".into(),
                text_start_bytes: 0,
                text_end_bytes: 6,
                kind: "final".into(),
            }
        )
        .unwrap()
    );
    finish_streaming(store.conn(), "failed", "result", "failed", Some(12)).unwrap();
    finish_streaming(store.conn(), "legacy", "result", "done", None).unwrap();
    let page = page_before(store.conn(), "t", PageCursor::Head, 20).unwrap();
    assert!(page.assistant_responses.is_empty());
    assert!(page.rows.iter().all(|row| row.content == "result"));
}

enum ResponseWriterEvent {
    Waiting,
    Finished(Result<bool, rusqlite::Error>),
}

thread_local! {
    static RESPONSE_WRITER_GATE: std::cell::RefCell<Option<(
        std::sync::mpsc::Sender<ResponseWriterEvent>,
        std::sync::mpsc::Receiver<()>,
    )>> = const { std::cell::RefCell::new(None) };
}

fn response_writer_contention(finish_owner: bool) {
    use std::sync::mpsc;
    use std::time::Duration;

    let (store, dir) = setup();
    insert(store.conn(), &streaming("response", 1)).unwrap();
    insert(store.conn(), &streaming("peer", 2)).unwrap();
    update_streaming_content(store.conn(), "response", "owned response").unwrap();
    let worker_store = Store::open(dir.path().join("vega.db")).unwrap();
    let writer = store.immediate_transaction().unwrap();
    if finish_owner {
        writer
            .execute(
                "UPDATE messages SET status = 'interrupted' WHERE id = 'response'",
                [],
            )
            .unwrap();
    } else {
        update_streaming_content(&writer, "peer", "concurrent peer").unwrap();
    }
    let (events, receiver) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        RESPONSE_WRITER_GATE.with(|gate| *gate.borrow_mut() = Some((events.clone(), released)));
        worker_store
            .conn()
            .busy_handler(Some(|_| {
                RESPONSE_WRITER_GATE.with(|gate| {
                    gate.borrow_mut().take().is_some_and(|(events, released)| {
                        events.send(ResponseWriterEvent::Waiting).is_ok()
                            && released.recv_timeout(Duration::from_secs(5)).is_ok()
                    })
                })
            }))
            .unwrap();
        let result = record_assistant_response(
            worker_store.conn(),
            "response",
            "owned response",
            &AssistantResponseRow {
                message_id: "response".into(),
                response_id: "round".into(),
                text_start_bytes: 0,
                text_end_bytes: "owned response".len(),
                kind: "commentary".into(),
            },
        );
        events.send(ResponseWriterEvent::Finished(result)).unwrap();
        worker_store.conn().busy_handler(None).unwrap();
        RESPONSE_WRITER_GATE.with(|gate| *gate.borrow_mut() = None);
    });
    let first = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
    let waited = matches!(first, ResponseWriterEvent::Waiting);
    writer.commit().unwrap();
    let _ = release.send(());
    let result = match first {
        ResponseWriterEvent::Waiting => {
            match receiver.recv_timeout(Duration::from_secs(5)).unwrap() {
                ResponseWriterEvent::Finished(result) => result,
                ResponseWriterEvent::Waiting => panic!("writer must wait at most once"),
            }
        }
        ResponseWriterEvent::Finished(result) => result,
    };
    worker.join().unwrap();
    assert!(
        waited,
        "response writer must wait before owner reads: {result:?}"
    );
    assert_eq!(result.unwrap(), !finish_owner);
    let count: i64 = store.conn().query_row(
        "SELECT COUNT(*) FROM assistant_responses WHERE message_id = 'response' AND response_id = 'round'",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(count, i64::from(!finish_owner));
    let owner = find(store.conn(), "response").unwrap().unwrap();
    assert_eq!(owner.content, "owned response");
    assert_eq!(
        owner.status,
        if finish_owner {
            "interrupted"
        } else {
            "streaming"
        }
    );
    if !finish_owner {
        assert_eq!(
            find(store.conn(), "peer").unwrap().unwrap().content,
            "concurrent peer"
        );
    }
}

#[test]
fn issue67_response_writer_waits_before_reading_and_preserves_peer_commit() {
    response_writer_contention(false);
}

#[test]
fn issue67_response_writer_revalidates_terminal_owner_after_waiting() {
    response_writer_contention(true);
}
