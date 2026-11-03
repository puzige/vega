use super::*;

#[test]
fn background_deletion_rejects_live_child_and_retries_after_worker_exit() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("deletion.db");
    let store = Store::open(&database).unwrap();
    store.migrate().unwrap();
    let parent = create_standalone_thread(&store, "model", "confirm").unwrap();
    let child = create_standalone_thread(&store, "model", "confirm").unwrap();
    vega_store::messages::insert(
        store.conn(),
        &vega_store::messages::MessageRow {
            id: "instruction".into(),
            thread_id: child.id.clone(),
            seq: 1,
            role: "user".into(),
            kind: "text".into(),
            content: "task".into(),
            status: "done".into(),
            created_at: 1,
            plan_status: None,
            plan_review_note: None,
            plan_reviewed_at: None,
        },
    )
    .unwrap();
    vega_store::subagents::insert(
        store.conn(),
        &vega_store::subagents::DelegationRow {
            child_thread_id: child.id.clone(),
            parent_thread_id: parent.id.clone(),
            parent_message_id: "parent-message".into(),
            spawn_call_id: "spawn".into(),
            child_message_id: "child-message".into(),
            name: "child".into(),
            task: "task".into(),
            state: "running".into(),
            result: String::new(),
            truncated: false,
            created_at: 1,
        },
    )
    .unwrap();
    let live = crate::subagents::LiveDelegation::new(&database, &child.id).unwrap();
    for target in [&parent.id, &child.id] {
        assert!(matches!(
            persist_thread_deletion(&database, target),
            Err(ConversationError::CorruptRow(_))
        ));
        assert!(get_thread(&store, &parent.id).is_ok());
        assert!(get_thread(&store, &child.id).is_ok());
    }
    drop(live);
    persist_thread_deletion(&database, &parent.id).unwrap();
    assert!(get_thread(&store, &parent.id).is_err());
}
