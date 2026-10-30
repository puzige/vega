use super::*;

fn fixture() -> crate::Store {
    let store = crate::Store::open(":memory:").unwrap();
    store.migrate().unwrap();
    crate::threads::create_standalone(
        store.conn(),
        crate::threads::NewThread {
            id: "thread",
            project_id: "",
            title: "timestamp",
            mode: "ask",
            permission_mode: "readonly",
            permission_revision: 0,
            full_access_grant: None,
            model: "test",
            status: "active",
            pinned: false,
            unread: false,
            created_at: 1,
            updated_at: 1,
            reasoning_preferences: None,
        },
    )
    .unwrap();
    insert(
        store.conn(),
        &MessageRow {
            id: "assistant".into(),
            thread_id: "thread".into(),
            seq: 1,
            role: "assistant".into(),
            kind: "text".into(),
            content: "".into(),
            status: "streaming".into(),
            created_at: 1,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .unwrap();
    store
}

#[test]
fn response_clock_is_idempotent_and_participates_in_outer_transaction() {
    let store = fixture();
    let tx = store.immediate_transaction().unwrap();
    assert_eq!(
        record_assistant_response_start(&tx, "assistant", "response", 0, 1000).unwrap(),
        1000
    );
    assert_eq!(
        record_assistant_response_start(&tx, "assistant", "response", 0, 2000).unwrap(),
        1000
    );
    tx.rollback().unwrap();
    assert_eq!(
        last_assistant_response_start(store.conn(), "assistant").unwrap(),
        None
    );
}

#[test]
fn invisible_boundary_and_response_commit_share_transaction() {
    let store = fixture();
    let tx = store.immediate_transaction().unwrap();
    assert_eq!(
        record_assistant_response_start(&tx, "assistant", "response", 0, 1000).unwrap(),
        1000
    );
    assert_eq!(
        record_timestamp_boundary(&tx, "assistant", "compaction", 0).unwrap(),
        0
    );
    assert_eq!(
        record_timestamp_boundary(&tx, "assistant", "compaction", 0).unwrap(),
        0
    );
    tx.commit().unwrap();
    assert_eq!(
        last_assistant_response_start(store.conn(), "assistant").unwrap(),
        Some(1000)
    );
    let count: i64 = store
        .conn()
        .query_row(
            "SELECT count(*) FROM message_timestamp_boundaries",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn retry_preserves_current_clock_but_discards_later_same_offset_response() {
    let store = fixture();
    record_assistant_response_start(store.conn(), "assistant", "first", 0, 1000).unwrap();
    record_assistant_response_start(store.conn(), "assistant", "later", 0, 2000).unwrap();
    let tx = store.immediate_transaction().unwrap();
    rollback_assistant_response_starts(&tx, "assistant", 0, Some("first")).unwrap();
    tx.commit().unwrap();
    assert_eq!(
        last_assistant_response_start(store.conn(), "assistant").unwrap(),
        Some(1000)
    );
    assert_eq!(
        record_assistant_response_start(store.conn(), "assistant", "first", 0, 9000).unwrap(),
        1000
    );
}

#[test]
fn rollback_moves_invisible_boundary_without_deleting_it() {
    let store = fixture();
    record_assistant_response_start(store.conn(), "assistant", "first", 0, 1000).unwrap();
    assert_eq!(
        update_streaming_content(store.conn(), "assistant", "body").unwrap(),
        1
    );
    record_timestamp_boundary(store.conn(), "assistant", "compaction", 4).unwrap();
    let tx = store.immediate_transaction().unwrap();
    rollback_assistant_response_starts(&tx, "assistant", 0, Some("first")).unwrap();
    tx.commit().unwrap();
    let offset: i64 = store
        .conn()
        .query_row(
            "SELECT text_start_bytes FROM message_timestamp_boundaries",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(offset, 0);
}

#[test]
fn manual_boundary_on_completed_message_restores_at_page_seam() {
    let store = fixture();
    assert_eq!(
        update_streaming_content(store.conn(), "assistant", "complete").unwrap(),
        1
    );
    store
        .conn()
        .execute("UPDATE messages SET status='done' WHERE id='assistant'", [])
        .unwrap();
    record_timestamp_boundary(store.conn(), "assistant", "manual-1", 8).unwrap();
    let page = page_before(store.conn(), "thread", PageCursor::Head, 20).unwrap();
    assert_eq!(page.timestamp_boundaries.get("assistant"), Some(&vec![8]));
}
